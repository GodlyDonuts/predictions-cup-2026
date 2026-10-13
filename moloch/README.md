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

The local bounded archive contains twenty authentic book records and no trade
receipts. It can check ingestion and quoting, but cannot establish queue fills,
spread capture, or superiority to another bot. A real trade/book corpus or local
read-only feed is needed for economic comparison. No competing account collector
is started by this implementation.

Next evaluate chronological development and untouched holdout periods with the
same funded ledger, fees, latency, liquidation depth, and unresolved liabilities.
Select parameters only on development data. Scale or deploy only after actual
prospective paper observations show net profit; no code test grants trading
authority or renews an old activation.
