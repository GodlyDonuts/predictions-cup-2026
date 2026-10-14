# Moloch

Independent paper market maker on `codex/moloch`. It has its own simulated wallet,
orders, journals, and policy. It neither controls Baal nor submits contest orders.
No Telegram coordination is required. Human instruction to use this branch
supersedes the old direct-main publication workflow for Moloch.

The event loop ranks current books by spread, observed activity, and capital use.
It joins bid/ask prices, preserves unchanged quotes, prioritizes covered inventory
exits, and limits funding, market concentration, and simulated write throughput.
Microprice is a heuristic, not a trained or validated forecast.

Cash and fees use integer micro-SUSQies. Counterfactual fills require a later
observed print executed after the three-second activation delay and at least one
tick through the quote. Participation defaults to 1% of observed volume, with
fractional credit accumulation. At-touch prints do not prove fills. Original
execution and receipt clocks stay separate; duplicate trade IDs cannot fill twice.

Cancellation becomes effective after modeled latency without freezing unrelated
markets. Its unresolved funding remains reserved until a complete original quote
window is projected through a post-end book checkpoint. Owned shares cannot be
pledged simultaneously to a sale and an outstanding complementary purchase.
Unknown data coverage does not turn into free cash. The independent wallet starts
with 1,000 simulated SUSQies, 40 committed total, 10 per market, and four markets.
These values are not contest-account allocations.

## Run

Requires Node 22+; no dependencies or API credentials.

```powershell
node --test moloch/engine.test.mjs moloch/run.test.mjs
node moloch/run.mjs --input data/maker-public-book-export-20261012.json --format book-export --output runtime/moloch/book-smoke-1
node moloch/run.mjs --input C:/market-data/events.ndjson --output runtime/moloch/paper-1 --follow --seconds 3600
```

Every run requires a new output directory. It writes a chained `events.jsonl`,
hashed `state.json`, and final `report.json`. For a tailing run, create `STOP`
inside that run's output directory to stop. Inputs are read only; no network calls
are made. NDJSON input must preserve source availability order; future events are
not sorted backward into earlier decisions.
File-following runs declare a wall-clock cutoff, skip earlier source events, and
use actual processing time for new orders. Original source clocks remain intact;
an old receipt read today cannot create an order yesterday.

Each event has `type` and `receivedAt` (original UTC receipt time).

- `book`: `book` contains tournamentId, marketId, exchangeId, requestStartedAt,
  receivedAt, sourceAt, engine sequence, and sorted bid/ask depth with decimal
  price strings and integer quantities.
- `trade`: tournamentId, marketId, exchangeId, stable engine id, executedAt,
  yesPrice, quantity. Do not substitute account-fill or unscoped legacy IDs.
- `gap`: exchangeId. It invalidates that book and drains active quotes.
- `coverage`: tournamentId, marketId, exchangeId, original from/to UTC window,
  complete=true, projectedThroughSequence. Only verified complete source windows
  belong here; isolated stream observations are not coverage.
- `tick`: an original processing clock event for quote expiry/cancel pacing.

Reports distinguish realized cash accounting from executable-depth liquidation.
Insufficient or stale liquidation depth produces null, not a fabricated mark.
YES/NO pair redemption is an explicit conditional paper payout assumption.
Positive synthetic tests establish accounting behavior, not profitability.

## Current evidence and next improvement

The initial local archive contains twenty authentic book records. The new official
SIG capture also retrieved current books/tape and all available hourly history for
market 322 / exchange 1011: 219 candles from October 1 through October 13, with
complete API coverage. See `results/sig-replay-20261013.json` for source hashes,
timings and forecast scores. This is one exchange, not the entire tournament.

Rust replay took 3.88 ms for those candles; the separate wallet process took
132 ms including Node startup and journaling. Rust handles chronological replay
and online forecasting; the financial engine remains JavaScript. The Rust time
does not measure wallet throughput. Sparse gaps are skipped, not fabricated.
The AR(1) forecast's prequential RMSE was .006007 versus .005810 for zero change;
it is not accepted as trading alpha. Candles have no historical bid/ask depth or
queue observations and cannot establish maker profitability.

The human confirmed the key shares Baal's account and then explicitly instructed
Moloch to recreate the feed if necessary. This authorizes this bounded read-only
capture as an exception to the old one-collector collaboration workflow. It does
not transfer contest execution ownership. One local mutex and persistent admission
record limit Moloch to five completed-paced reads/minute. Realtime capture uses
one ephemeral token request per bounded run, at most one per minute; it sends no
contest orders.
The shared server account limit still applies; a request failure stops capture
and persists a blocked state. HTTP status and minimum backoff are saved in
`capture/failure.json`. A bounded diagnostic probe can establish recovered service
before the operator clears that block. Never restart by merely deleting the block.

Moloch's running paper episodes have their own immutable source/code manifests.
Resting quotes retain their declared lifetime despite the ten-second entry
deadline, while explicit gaps and sixty-second source silence drain them. Their
funding is still held until complete original quote-window coverage and a post-end
checkpoint arrive. A named successor carries exact cash, inventory, orders,
reservations and deduplication state; it adds zero paper capital.

## Official SIG capture and Rust replay

The existing approved DPAPI credential is read in memory by PowerShell; it never
appears in command arguments, capture payloads or Git. Raw receipts and paper
state stay under ignored `runtime/` directories. The source is the documented
[SIG API v1](https://sig.thesuper.market/api/v1/docs), with explicit tournament scope.

```powershell
# This device's isolated Rust installation; no global PATH change required.
$env:RUSTUP_HOME=Join-Path $env:LOCALAPPDATA 'Codex/tools/moloch-rustup'
$env:CARGO_HOME=Join-Path $env:LOCALAPPDATA 'Codex/tools/moloch-cargo'
& "$env:CARGO_HOME/bin/cargo.exe" test --manifest-path moloch/native/Cargo.toml
& "$env:CARGO_HOME/bin/cargo.exe" build --release --manifest-path moloch/native/Cargo.toml

./moloch/capture-sig.ps1 -MarketId 322 -ExchangeId 1011 -OutputDirectory runtime/moloch/history-new -HumanAuthorizedSharedReadCapture -HistoryOnly -AllHistory
./moloch/native/target/release/moloch-replay.exe --input runtime/moloch/history-new/history.json --format history --output runtime/moloch/history-new/events.ndjson --stats runtime/moloch/history-new/rust.json
node moloch/run.mjs --input runtime/moloch/history-new/events.ndjson --output runtime/moloch/history-new/wallet

# Bounded prospective paper run: at most 100 cycles, approximately forty minutes.
node moloch/live.mjs 322 1011 runtime/moloch/live-new 100
# Continue from a verified terminal paper episode without resetting finances.
node moloch/live.mjs 322 1011 runtime/moloch/live-next 100 runtime/moloch/live-new
```

Use a new directory for every episode. `live.mjs` owns one capture subprocess and
one paper writer; its one-second heartbeat generates no API traffic. Source
receipt clocks and actual processing clocks remain separate. `stats.json` reports
successful official receipts, connection evidence, modeled orders/fills, realized
cash and executable liquidation. The live connection is REST polling, not a
websocket or an HFT latency claim. Place `STOP` in the episode directory to stop.

Native `--format book-export` validates the bounded source checksum and record
chain. `--format events` replays normalized NDJSON. It rejects backward clocks and
does not sleep. `--format history` emits only completed candle closes at their
bucket ends, explicitly assuming historical availability then; actual historical
reporting latency cannot be reconstructed from today's history response. Complete
single-page tape windows can carry coverage; isolated cursor pages cannot release
quote reservations.

Check both language implementations and their financial parity:

```powershell
node --test moloch/engine.test.mjs moloch/run.test.mjs moloch/sig-source.test.mjs moloch/native-parity.test.mjs
```

Next evaluate chronological development and untouched holdout periods with the
same funded ledger, fees, latency, liquidation depth, and unresolved liabilities.
Select parameters only on development data. Profitable prospective observations
are needed before proposing scale. Contest deployment additionally requires fresh
human authorization and sole execution authority for Moloch; the earlier Baal
activation or conditional instruction is not silently transferred to this bot.

## Market exploration

The October 13 prospective control episode completed 201 official receipts with
zero modeled fills. Its large displayed queues motivate searching other markets;
touch prints are still not counted as fills. Catalog metadata last prices do not
describe executable spreads or current activity.

```powershell
./moloch/capture-sig.ps1 -CatalogOnly -OutputDirectory runtime/moloch/catalog-new -HumanAuthorizedSharedReadCapture
node moloch/screen.mjs select runtime/moloch/catalog-new/receipts.jsonl runtime/moloch/catalog-new/pairs.json
./moloch/capture-sig.ps1 -ScreenPairsFile runtime/moloch/catalog-new/pairs.json -OutputDirectory runtime/moloch/screen-new -HumanAuthorizedSharedReadCapture
node moloch/screen.mjs report runtime/moloch/screen-new/receipts.jsonl runtime/moloch/screen-new/report.json
```

Catalog cursor completeness is checked before selecting a fixed index sample.
The screen ranks spread, recent prints and displayed queue turnover for further
research; sequential book/tape observations cannot establish fill probability or
profit. Incomplete tape windows are excluded from ranking. Raw captures remain
private under ignored runtime directories.

Live continuation can add one candidate using a JSON pair file as the seventh
argument after the parent directory. It must retain the original market and every
prior declared scope. Both books and tapes use the same serialized five-read
budget; two markets take roughly 48 seconds per cycle. The scopes are copied into
the new immutable manifest. Cash, reservations, inventory and historical orders
carry forward with zero new capital. No additional account client is started.
Run `node --test moloch/*.test.mjs` for the complete Node checks.

The default flat-entry policy now evaluates the combined spread after both legs'
modeled fees, rather than using uncalibrated microprice imbalance as directional
alpha. A new pair must fit cash and market limits together; scarce funding cannot
create only its first leg. Actual fills can still leave unpaired inventory, which
uses the existing covered-sale risk control. `quoteMode: microprice` remains an
explicit comparison policy. A resumed legacy ledger receives the new default
policy in its successor manifest without rewriting its historical quotes.

Restarted paper episodes request the original windows of expired unresolved
quotes once, before prospective captures. Only complete tape coverage and a later
scoped book checkpoint can release their reservations. A paginated recovery
response leaves funding unresolved; restarting never releases it by wall time.
The [October 13 market screen](results/sig-screen-20261013.json) selects FL-07 for
an unseen prospective test alongside the existing FL-09 control.

## Recorded-clock intervention tests

```powershell
node moloch/compare.mjs runtime/moloch/live-completed runtime/moloch/live-completed/comparison.json
```

This requires a completed successor episode, verifies its parent snapshot and
entire event hash chain, and reproduces the baseline financial state exactly
before evaluating early quote renewal. It preserves recorded processing clocks,
execution timestamps and existing liabilities. Variant fills are hypothetical;
the same observations are development data and cannot serve as a holdout.

The [October 13 renewal comparison](results/sig-renewal-20261013.json) reproduced
the completed two-market run: 25 official receipts, zero fills and zero realized
profit. Renewal 30/45/60 seconds before expiry each produced one hypothetical NO
purchase with a later fresh liquidation mark of -0.005, no realized profit and
one remaining position. Final executable liquidation was unknown because its book
had aged past the entry deadline. Faster renewal therefore remains disabled by
default (`renewBeforeMs: 0`); increased fill count alone is not economic success.

The current official realtime documentation supports private tournament market
topics carrying versioned books and trades in one-second batches. It requires an
ephemeral three-hour token from `POST /realtime/token` (one account-budget write),
initial/periodic REST synchronization, and resynchronization on gaps or truncated
batches. The capture integration below obtains these observations; the financial
paper runner still uses REST polling. Neither establishes gap-free HFT data.

## Realtime capture

```powershell
# Install only the pinned isolated realtime client; install scripts are disabled.
pnpm --dir moloch/realtime install --frozen-lockfile --ignore-scripts
./moloch/capture-sig.ps1 -ScreenPairsFile runtime/moloch/screen-new/live-pairs.json -OutputDirectory runtime/moloch/stream-new -Cycles 6 -HumanAuthorizedSharedReadCapture -RealtimeCapture
```

This mode adds a private tournament market subscription for each of at most four
configured markets, using the documented Supabase client. The existing capture
process retains its mutex and owns all REST and authentication calls. It admits
at most five local reads and one authentication write per minute, mints one
three-hour token for the bounded episode, and never refreshes it automatically.
The human's real-SIG-feed/recreate instruction supplies authorization for this
feed authentication; it grants no contest order authority.

The token and server connection keys pass directly to the helper's standard input
and remain in memory. Authentication responses are omitted from all journals,
arguments, environment variables and logs. The helper captures only validated
public book/trade fields, records revisions/gaps/truncations, and cross-checks
periodic authoritative REST snapshots. Socket/channel failures end the capture;
they do not trigger credential renewal or a silent fallback. `STOP` in the capture
directory gracefully ends the bounded parent and its stream helper.

`stream/events.jsonl` is hash chained. `stream/stats.json` reports subscriptions,
source observations and trade latency when trades actually occur; book arrival
alone cannot establish execution latency. A startup manifest pins local code and
the dependency lock. The first live probe recorded its identity after startup,
before subsequent edits, and archived those exact source bytes separately.

The capture helper places **zero paper or contest orders**. Its observations can
now feed the carried paper wallet through the integration below. All previous
ledger histories remain preserved; no contest execution path is added.

## Stream paper wallet

```powershell
node moloch/live.mjs 322 1011 runtime/moloch/live-stream-new 6 runtime/moloch/live-stopped runtime/moloch/screen-new/live-pairs.json --realtime
```

This keeps the same simulated cash, inventory, reservations, historical orders
and deduplication state. One collector owns the stream authentication and REST
queue. Pushed executions are evaluated against existing quotes before their newer
book is applied. Different fractional timestamp serializations of the same
execution deduplicate without changing legacy financial hashes; changed price,
quantity or instant still stops the ledger.

Observed SIG REST tape timestamps can be rounded to milliseconds while pushed
executions retain sub-millisecond precision. Cross-feed duplicates use the same
documented engine match ID, exact market/price/quantity and millisecond bucket;
this tolerance applies only when the REST representation has no sub-millisecond
component. Both original wire clocks remain in the evidence, and only a duplicate
reuses its first verified representation. New executions retain their actual wire
clock. Fine-precision disagreements, changed prices/quantities and different
millisecond buckets still fail closed.

On continuation the merge reconstructs first-observation facts from verified
predecessor event chains and exact stored deduplication hashes. No stored financial
hash or cash balance is rewritten. Preserve these ignored runtime journals: absent
or altered provenance stops startup before any authentication request.

Initial subscription and gaps block new admission until a scoped authoritative
book and complete tape window spanning the invalidation arrive. Older REST books
cannot replace newer pushes. A trade-tape request cannot refresh a book's clock.
Pushed books age from their engine `asOf` clock, not their arrival time. Their
quote-end barrier also uses that source clock, so delayed pushes cannot release
cash merely by arriving later.

Historical recovery now reads a post-end book before requesting its quote-window
tape. While current continuity remains blocked, this authoritative book can form
a financial checkpoint with `invalid: true`, which permits no new quote. Only
matching complete coverage releases old funding. This corrects the first stream
episode's stranded historical reservations without resetting its wallet.

Local source bytes are archived with each new episode. Stream helper status is
written atomically; explicit stream failures stop paper admission and drain the
same collector rather than silently downgrading or refreshing its credential.
The fill model remains strict one-tick-through with bounded participation; all
fills are counterfactual, and realtime connectivity is not profitability evidence.

The [October 13 carried-wallet stream test](results/sig-stream-wallet-20261013.json)
records the initial reservation-ordering finding, the precision-conflict stop,
and a corrected terminal run with both subscriptions verified, all predecessor
orders retired, 12 REST reads, one authentication write, unchanged 1000.000000
cash, four remaining modeled quotes and zero fills/profit. Reapplying 11 captured
duplicates reconciled four precision differences without changing cash, realized
PnL, fills or stored deduplication hashes. The next economic step is selecting
markets from fresh tournament-wide flow rather than keeping two mostly quiet
markets solely because an earlier short sample was active.

## Tournament activity discovery

```powershell
./moloch/capture-sig.ps1 -CatalogOnly -OutputDirectory runtime/moloch/catalog-next -HumanAuthorizedSharedReadCapture
node moloch/screen.mjs universe runtime/moloch/catalog-next/receipts.jsonl runtime/moloch/catalog-next/universe.json
./moloch/capture-sig.ps1 -ScreenPairsFile runtime/moloch/selected-rest-pairs.json -UniversePairsFile runtime/moloch/catalog-next/universe.json -OutputDirectory runtime/moloch/universe-next -Cycles 6 -HumanAuthorizedSharedReadCapture -RealtimeCapture
node moloch/activity.mjs runtime/moloch/universe-next/stream/events.jsonl runtime/moloch/universe-next/activity.json
```

Universe observation uses one private tournament `markets` topic, never alongside
per-market topics. The catalog is cursor-verified and validated before auth.
Unknown catalog markets trigger a refresh notice rather than admission. Only the
selected REST scopes are synchronized; the other markets are observational, not
execution-ready. Public normalized fields and per-market revisions are preserved.

Activity analysis verifies the source hash chain, tracks each exchange separately,
and exposes last-three-minute flow and current spread/queues. Thirty-second
conditional one-share markouts require a previously observable, stable price
regime with three seconds of placement lead and a subsequent executable book.
They are not filled-wallet returns. Gaps exclude direct observation-based ranking,
and stale books cannot authorize entry. Exploratory candidates are read again over
official REST before a prospective paper test.

The [October 13 universe probe](results/sig-universe-20261013.json) observed 142
trades and 262 gaps, affecting 193 markets. Its 6,319 rows passed chain validation;
no complete-tape or profitability claim follows. Scoped REST then verified
Nebraska Democratic Senate (279/968) and Republican MN-01 (335/1024) as additional
paper candidates. Hawaii's larger flow came with much deeper queues. An unchanged
carried wallet follows the two candidates plus the two historical controls using
four per-market topics. REST-only mode retains its two-market limit. With four
stream scopes REST cycles take roughly 96 seconds; the stream remains the timely
book source, and cycle counts are bounded to the helper's lifetime.

## Competitive quote experiment

`insideTicks` defaults to zero. An explicit prospective paper experiment may pass
`1` after `--realtime` in the live command. It carries all finances and historical
quotes into the successor; the new policy applies only to new decisions.

Improvement stays inside the observed spread, leaves at least one tick between
the bid and ask, preserves combined fee/edge margin and fits both funded legs.
Depth-weighted microprice ranks which leg can afford improvement; this remains an
uncalibrated heuristic. Balanced wider books can improve both legs, while a
two-tick book can improve only the leg with sufficient modeled depth-value edge.
Covered inventory exits may improve their price only while retaining modeled
round-trip fee coverage and the minimum edge against the worst owned lot basis.
If that improvement would erase the edge, the original passive exit remains;
adverse moves can still lose money. No fill rule or reserve-release rule is relaxed.

The [recorded-clock comparison](results/sig-inside-20261013.json) exactly reproduced
the four-market baseline. Both join and improved variants produced zero fills on
those development events, so neither establishes profit. The separate subsequent
live experiment tests the changed policy on unseen events with zero new capital.

The current official [order contract](https://sig.thesuper.market/api/v1/docs),
rechecked at 2026-10-13T23:38:20Z, explicitly states that opposite BUYs on
engine-routed scopes net the existing position at FIFO cost and credit one per
netted share. The model's internal paired-credit bookkeeping implements that
same-exchange mechanism, not a cross-market settlement bet. New offset records
are labeled `engine_opposite_buy_fifo_netting` and counterfactual; the historical
array name `redemptions` is retained for ledger continuity. Documentation confirms
the mechanism, not any actual fill, account profit or execution authorization.

## Queue sensitivity and market rotation

The default fill model remains `strictThrough`. An offline `conditionalFifo`
scenario starts behind visible equal/better-priced depth. Equal-price additions
before activation can go ahead; later additions go behind under the assumed FIFO
priority. Cancellation reductions are not credited. Better-priced depth remains
ahead until a conditional print reaches or passes the quote level. Touch volume
must first exhaust same-price queue, and residual volume still obeys the one-percent
participation cap. `touchAttributionBps` tests uncertain trade direction, not an
observed maker fill. Conditional scenarios cannot trigger `profitSignalEligible`.

The [queue replay](results/sig-queue-20261014.json) verified baseline financial
parity in three recorded episodes. Both 50% and 100% touch attribution still
produced zero fills. This implicates queue size and selected-market flow rather
than proving that a more permissive fill assumption would make money.

`--drain` after the live command's numeric inside-tick argument disables new buys
and cancels old buys without freeing their reservations. Covered inventory exits
remain available. The completed drain carried 1000.000000 cash, zero new capital,
zero inventory and zero unresolved/reserved funds. It did not pause the mission.
New paper scopes may replace old scopes only after all their orders are retired
and owned shares are zero. Historical books, fills and deduplication remain; all
retired books are invalidated together before any new admission. `--resume-entries`
is required to re-enable a drained successor. No contest owner is transferred.

Activity reports now expose `reconciliationPriority` from observed flow even for
markets with historical gaps, together with `quoteReadyFromObservation: false`
and mandatory authoritative recovery. These are exploration priorities, not
permission to quote from stale or incomplete data.

The [October 14 rotation](results/sig-rotation-20261014.json) recovered four
high-activity candidates with historical gap warnings through complete scoped
REST windows, then replaced the flat old scopes atomically. North Carolina Senate,
Texas Senate, New Mexico Governor and U.S. Senate ran on their per-market topics
with unchanged 1000.000000 cash and zero new capital. The run completed with zero
fills/profit; 34.825000 remained reserved for unresolved modeled quotes.

Activity diagnostics separate volume compatible with a prior observable bid from
volume compatible with its ask, and show the weaker side's queue turnover. This
requires the book to predate the execution and be received before it; a post-trade
book cannot invent earlier liquidity. Compatibility is not aggressor attribution.
High print counts with little share volume do not imply adequate maker turnover.

The [lifetime/participation sensitivity](results/lifetime-participation-20261014.json)
compares 48 recorded-clock variants across four episodes. Higher assumed
participation produced one entry in the last episode, but no profitable exit.
The entry's negative bid-liquidation mark reflects the spread; it does not alone
show adverse price movement. Default settings remain unchanged.

A prospective successor may use `--policy FILE` after the positional arguments.
Only `participationBps`, `lifetimeMs`, `quantity`, `retainProfitableHedge` and
`maximumHedgeOvershoot` are accepted. The first longer
window tests 10% hypothetical participation, ten-minute quotes and five-share
size under strict trade-through fills. Existing quotes retain their own
participation assumptions and expiration times; no carried capital is reset.
This setting is an experiment, not a measured fill probability or deployment gate.

The first longer successor stopped during historical recovery before new
quotes or fills. [Resume provenance evidence](results/resume-provenance-20261014.json)
records the cause: normalized reporting duplicates had replaced canonical stream
provenance during predecessor scanning. Resume now selects original verified
records and rejects missing originals. The stopped episode remains archived,
and its wallet carries into a separate successor without added capital.

Future live successors support `--universe FILE` alongside `--policy FILE`. The
universe must contain every financial scope and unique valid exchange IDs. Its
normalized contents and hash are frozen in the episode before capture starts.
The collector uses the single aggregate tournament channel, while serialized
REST reconciliation and financial admission remain limited to the declared four
markets. Other broadcasts stay in the observation journal for activity research;
they cannot create quotes, inventory or financial trade facts. This lets discovery
continue during position management without a separate collector or token.

The [forward maker window](results/forward-maker-20261014.json) observed 41 unique
future prints totaling 2954 shares across two active markets, but none traded
strictly through an eligible paper quote. Even the conditional FIFO sensitivity
produced no fills. Raising participation therefore did not create turnover.

[Aggregate discovery validation](results/aggregate-discovery-20261014.json) replays
8230 verified rows across 237 markets while keeping four financial scopes. The
prior capture had no two-sided compatible-flow candidate. Discovery now ranks
two-sided volume with room for inside quotes and can evaluate locally gap-free
segments after a fresh observable book; it retains every historical gap warning.
A live aggregate attempt stopped on a filesystem EPERM before new quotes. Only
status-file rename sharing conflicts get bounded retries; data/journal failures
remain fatal, and allowlisted failure phases now identify the filesystem operation.

The [first closed paper pair](results/first-closed-pair-20261014.json) netted five
North Carolina Senate YES/NO shares costing 4.975000 for a modeled 5.000000 credit,
realizing +0.025000. Both later official tape prints traded one tick through the
earlier quotes, after activation and before modeled cancellation. A 1%
participation sensitivity produces the same result. These are counterfactual
resting-quote fills during a feed outage, with unknown intermediate drawdown;
they are not contest fills or proof of scalable profit.

The aggregate run ended on HTTP503 with its paper wallet flat and no remaining
reservation. A single HTTP200 diagnostic book read after the preserved backoff
verified service recovery before clearing the blocked flag without resetting
admissions. Its successor tests 1% participation and ten-share paired packets
inside the unchanged 40 total/10 per-market funding caps, with zero added capital.
Discovery includes one-tick joins and ranks the weaker side's compatible queue
turnover; inside-quote room is reported separately.

The optional [retained-hedge study](results/retained-hedge-study-20261014.json)
keeps an already funded opposite quote after an entry, provided its worst owned
basis, quote price and modeled fees leave the required edge. It creates no new
hedge-buy orders. The default retains no such quotes. With the residual limit
absent or zero, all remaining opposite quantity must be owned and unpledged.

An explicit `maximumHedgeOvershoot` can permit a bounded remainder from that
original quote, without enlarging capital caps. The next prospective experiment
uses three residual shares for a ten-share packet, 1% participation and ten-minute
quotes. If ten opposite shares fill against seven owned shares, seven net and
three remain directional inventory. Their loss can erase the paired gain. Gaps,
source silence, canceled quotes and explicit drains remain controlling.

The ten-share predecessor completed with two new partial Texas Senate fills,
seven owned NO shares costing 4.270000 and no added realized profit. Its existing
+0.025000 gain is separate. Recorded-clock retention variants changed one cancel
but produced no added closed profit; all strategies had a stale final liquidation
mark. The successor carries those shares and reservations without resetting cash.

[Discovery and selective rotation](results/discovery-rotation-20261014.json)
corrects delayed-trade analysis by selecting the book actually observable before
execution from a bounded 64-snapshot history. A newer book received after that
execution cannot hide an earlier valid observation. Source age and local gap
generation remain mandatory. The same 15182-row archive now produces 224
conditional markouts instead of 185, without creating any wallet fills.

Optional `entryExchangeIds` restricts fresh entries to a subset of the declared
financial capture scopes. Excluded buy quotes cancel, reservations wait for
complete coverage, and owned covered exits stay managed. This retired flat New
Mexico Governor and U.S. Senate scopes while preserving North Carolina and Texas
inventory. Their successor adds Tennessee Senate after selecting two-sided
compatible volume sufficient for at least one assumed share at 1% participation.
Its initial REST book/tape reconciliation remains mandatory; discovery cannot
authorize quotes. All cash, positions, intents and trade deduplication carry.

[Optional aged inventory exits](results/aged-aon-exits-20261014.json) add
`inventoryTakerAfterMs`. After that age from the latest owned-side acquisition
receipt, a covered exit may use a marketable limit only when its bid-side price
covers worst owned basis and both modeled fees. Unknown passive-sale pledges must
reconcile first. The default stays passive.

The current official contract supports a single leg through `/orders/multi-leg`
with `executionMode: allOrNone`. This simulation requires the whole requested
quantity in a later fresh book after modeled latency, within a ten-second source
confirmation window. Insufficient depth models no execution and no resting
remainder. Consumed depth cannot be reused at the same side/engine sequence.
Each intent consumes its own paced write admission; no mixed maker/AON request
is assumed. Payout uses the limit-price lower bound and is never a claim of an
official fill. Original strict-print profit eligibility excludes these simulations.

The completed three-market replay freed 5.020000 principal through two hypothetical
break-even exits and added no realized profit. A fresh successor tests the ten-minute
age threshold with unchanged cash, holdings, attribution and funding caps. Public
depth cannot establish actual account self-trade exclusions or execution ownership.

[Strict-through source-volume sensitivity](results/through-capacity-sensitivity-20261014.json)
adds optional `throughFillCapacity` for diagnostics. New flagged quotes can
allocate up to the observed matched quantity after a one-tick trade-through,
activation and expiry checks. Touches do not qualify; only one quote receives
a fill per print. Legacy quote assumptions stay unchanged. This is not proof
of actual priority, impact or self-trade eligibility and is excluded from
strict-print profit certification. The field stays off in prospective runs.

The recorded inside-entry cohort took on more unclosed inventory. Full source
capacity worsened its liquidation loss, and neither variant added realized
profit. The next independent capture therefore joins the existing best prices.

`capture-sig.ps1 -LeaderboardOnly` makes two serialized, budgeted official
reads for all-time and current UTC-day P&L rankings. It retains only ranking and
financial metrics, omitting profile identities. Caller rank belongs to the shared
SIG account and cannot be attributed to Moloch's independent paper bank.

[Leaderboard benchmark](results/leaderboard-benchmark-20261014.json) captures
all-time and current UTC-day P&L rankings without participant identities. P&L
and volume are reported metrics, not proof of realized cash. The documented ROI
denominator is period volume, not starting capital. These account rankings do
not measure Moloch's independent paper wallet.

Optional `quoteMode: cheapSpread` quotes only the cheaper outcome below
`maximumEntryPrice` (default .100000), then manages fully owned inventory through
covered sales. Round-trip modeled fees must leave the required edge. It does not
reserve an expensive complementary entry and does not treat low price as evidence
of undervaluation. A controlled .045/.050 test quotes 100 cheaper shares using
4.500000 reservation and closes 50 for .250000 simulated gain; this is a behavior
test, not observed market profitability. Funding caps remain 40 total/10 per market.

The first prospective cheap-spread experiment carries the existing North Carolina
and Tennessee holdings and adds two lower-price observation candidates, after
retiring flat Texas. Their recent flow was one-sided, so this is exploratory
market research with mandatory REST recovery, not a validated positive signal.

[Chronological policy comparison](results/chronological-policy-20261014.json)
carries each hypothetical wallet across eight verified episodes without resets.
Replaying the recorded policy reproduces every terminal financial snapshot.
Joining rather than improving prices reduced the last fresh modeled loss from
.050000 to .025000. Cheaper entries preserved the inherited .025000 gain but
added no realized profit. Neither conditional queue assumption established an
edge. These previously inspected periods are retrospective diagnostics, not
pristine holdouts; training-only ranking cannot make them prospective evidence.

`capture-sig.ps1 -ExecutionHistoryOnly` reads one page of at most 100 own fills
and up to three associated orders through the same serialized read budget.
Use an ignored local output directory. This private account evidence must never
be published or imported as Moloch holdings. Pagination may remain incomplete;
coverage metadata does not mean this first page is the entire history. Fill
prices are outcome-denominated, and signed quantity does not identify buy/sell.
An order's limit and creation time alone cannot prove passive liquidity or
queue priority. This mode sends no orders and obtains no realtime token.

[Current-price discovery correction](results/current-price-discovery-20261014.json)
restricts compatible queue turnover to the current bid/ask regime, the current
gap generation, and executions after the three-second observation lead.
Raw recent activity remains visible for research. Moving away from a price and
returning does not reuse its old executions. In the completed 237-market capture,
four markets lose obsolete compatible volume and one loses a false two-sided
priority. This corrects candidate discovery; it does not establish profitable fills.

The subsequent [per-side observation check](results/per-side-discovery-20261014.json)
tracks bid and ask activation histories separately. A changed ask cannot erase
activity at a persistent bid, and a changed bid cannot erase persistent ask flow.
Changing that side's price or observing a gap still resets its history. On the
same frozen eight-minute prefix this recovered compatible activity in three
markets and one conditional markout, but no two-sided opportunity. Conditional
price marks remain research evidence, not paper-wallet earnings.

The [official competition rules](https://predictionscup.com/rules/), checked
October 14, rank final prizes by SUSQie balance after all markets resolve.
Participants start with 100,000 SUSQies; Moloch's separate 1,000-unit research
wallet is not a contest-account allocation. Trade count alone is not the objective.

[Optional maximum inventory age](results/maximum-inventory-age-20261014.json)
adds `maximumInventoryAgeMs`, measured from the latest owned-side acquisition
receipt. Once reached, a covered all-or-none sale may accept a loss at the
executable bid-side limit. The existing break-even age option is unchanged.
Neither age guarantees an exit: unresolved pledges, missing fresh depth, or
insufficient whole-order depth can delay it. New maximum-age intents and fills
record their reason, and each uses the existing paced modeled write budget.

The retrospective replay closed both old positions for 2.275000 proceeds,
leaving cash 999.950000 and realized loss .050000. This released inventory risk
without increasing liquidation value or demonstrating alpha. The prospective
drain policy uses thirty minutes and carries the actual paper wallet unchanged.
