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
record limit Moloch to five completed-paced reads/minute, no token/order writes.
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

This mode adds a private tournament market subscription for each of at most two
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
