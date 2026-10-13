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
