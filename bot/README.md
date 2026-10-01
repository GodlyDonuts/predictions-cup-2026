# Read-only SIG bot research

Node.js 22+ scanner for the 2026 Midterm Elections tournament, using only the supported API v1. No third-party packages required. **No order placement, cancellation, or autonomous trader is enabled.** The client rejects all non-GET requests; `--live` is rejected before credentials are loaded.

dot owns execution under the repository coordination protocol. This scanner prepares evidence for that executor and must not be treated as an ownership handoff.

## Run

```powershell
node --test
node bot.mjs --once --sample # focused check of the first 20 eligible contracts
./Start-Bot.ps1 -Once   # one read-only scan
./Start-Bot.ps1         # repeated read-only scans
./Stop-Bot.ps1          # stop after current requests complete
```

Run from this directory. The first scan fetches structured metadata for all eligible contracts and takes several minutes. Later scans use metadata cached for up to 12 hours. Remove the local `STOP` file before a restart. A per-Windows-user lock prevents duplicate local scanner instances; it is not a cross-host execution lease. On a crash, check the PID in `%LOCALAPPDATA%\SigTradingCup\bot.lock` before removing a stale lock.

If `STOP` is present at startup, the scanner exits before reading configuration, loading credentials, creating runtime output or making any API request. A stopped monitor must not bypass this control to collect data.

## Credential

The supplied key is stored outside Git and OneDrive, encrypted with Windows DPAPI under `%LOCALAPPDATA%\SigTradingCup\api-key.dpapi`. It decrypts only in memory under the same Windows user. On another machine provide `SIG_API_KEY` through approved secret storage. Never commit keys, raw auth headers, or private account snapshots. Requests restrict authentication to `https://sig.thesuper.market/api/v1` and reject redirects.

## Candidate strategy

Opposing party-winner contracts with matching structured race, stage, election date, and valid settlement date are **unverified metadata hypotheses**. Matching metadata does not prove single-seat rules, exclusive winners, or a settlement truth table. Fusion or joint endorsement could allow both YES contracts to settle positively, losing both NO legs. The scanner publishes no minimum payout or minimum profit and cannot emit an executable structural signal. Exact source-backed resolution rules and all possible settlement outcomes must be checked before any hedge claim.

Hypothetical NO quote cost is the complement of a YES bid, rounded upward to the 0.005 tick. The scanner requires a quote difference of at least 0.02 from one unit; this is a filter, not an expected return or guaranteed edge. Research sizing uses at most 25% of top-level depth, 250 shares per leg, and 250 SUSQies per package, preserving an 80,000 cash reserve. Existing holdings or any own open order on either exchange exclude the pair entirely. An open order with an unknown exchange excludes all pairs conservatively. This prevents our own resting liquidity from supporting a quote estimate. Account and book observations can still become stale.

Cost accounting uses integer thousandths and decimal strings. Candidate records carry `classification: unverified_metadata_hypothesis`, `settlementVerified: false`, and `eligibleForExecution: false`. Historical records from older scanner versions must not be interpreted as verified signals.

Candidates are **not approved for execution**: sequential books can be stale, fees are unverified, orders can fill unequally, and refunds/rule changes can invalidate the relationship. API multi-leg placement is atomic admission, not a guarantee of equal fills. A future executor needs fresh complete account reconciliation, verified rules/fees, a real shared lease, a central per-account limiter, persisted intents, uncertain-result recovery, and bounded partial-fill handling before enabling any orders.

## Observations and rate budget

- `data/status.json`: timestamp and latest completed read-only scan.
- `data/portfolio.json`: timestamped account positions and observed cash.
- `data/orders.json`: historical open-order observation.
- `data/pairs.json`: unverified metadata matches.
- `data/opportunities.json`: unverified quote hypotheses, limitations, book versions and observation times.
- `data/sources/`: append-only source responses with request/receipt timestamps.
- `logs/bot.jsonl`: scanner progress and findings; no credentials.

Runtime files stay local and are ignored by Git. Snapshots are historical observations, not live promises. Price quotes should never be executed from these files without fresh revalidation.

REST admissions are spaced 1.5 seconds apart (at most about 40/minute), with four network reads in flight during metadata discovery. Cooldowns and retries use the same serialized admission queue, maintaining spacing after a pause; Retry-After supports delta seconds and HTTP dates with a finite fallback. This limiter applies to this process only; every agent and key shares the account's 100-read/30-write budget. It leaves headroom but does not coordinate another host. Agree on allocations in issue #1 before concurrently running other high-rate clients. The research implementation does not yet use realtime feeds or perform replay/backtesting of a complete tournament strategy.

There is no evidence yet that this narrow strategy will win or even find sufficient opportunities. External fair-value forecasts and correlated portfolio risk research remain necessary for a broader competition strategy.

## Sources

- [Official rules](https://predictionscup.com/rules/): bots allowed; final rankings after all market settlements.
- [Trading guide](https://sig.thesuper.market/docs/markets-and-trading): peer-to-peer execution and complementary YES/NO shares.
- [API reference](https://sig.thesuper.market/api/v1/docs): tournament scopes, tick size, depth, limits and multi-leg semantics.

Documentation reviewed October 1, 2026. Current limitations and validation results belong in the draft PR and coordination issue #1.
