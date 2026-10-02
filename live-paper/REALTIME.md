# Realtime market capture

The single collector can subscribe to the four configured tournament markets through the official API v1 Supabase private broadcast feed. This records market books and trade observations in a separate, restricted local session journal. Official orders continue to use the existing reconciled REST-book controller and its existing pilot limits.

Install the pinned root dependency with `pnpm install --frozen-lockfile`. The normal collector launcher remains `Start-Paper.ps1`. An approved runtime `realtime-capture.json` containing `{"schemaVersion":1,"enabled":true,"mode":"market_capture_only"}` enables capture on the next collector start. This file belongs under the restricted paper runtime, never the repository.

`realtime-client.mjs` obtains the documented three-hour ephemeral connection token through the same account dispatcher as every other request. The token and SDK objects remain private in memory. It subscribes only to the configured tournament market topics, with no user/account channel. Authentication refresh and subscription recovery require a fresh REST snapshot. Socket retries are paced to one minute; connection failures do not cause an independent API polling loop.

`realtime-host.mjs` allows at most one depth-200 REST recovery per collector cycle, after the regular account/book work. Both the token request and recovery reads retain the shared serialized admission, persistent rate backoff, and final STOP checks. `REALTIME_STOP` stops the feed alone; the existing collector/global STOP files also apply. Recovery does not bypass the existing REST budget or submit an order.

`realtime-books.mjs` validates tournament/exchange identity, full versioned books, integer depth, tick prices, local receipt order and bounded source-clock skew. It distinguishes topic revisions from engine book versions. Revision gaps, compact batches, unsequenced trades, socket errors, expiry and staleness block readiness until recovery. A repeated topic revision can still carry a newer book. A duplicate book cannot extend freshness. A known pushed expiry survives an equal-version REST snapshot that omits expiry information. Settlement remains blocked pending authoritative market-state work.

The local `/api/realtime` route and `realtime-stats.json` expose sanitized connection and recovery health. `realtime-session.json` identifies the current exclusive capture file. Session journals contain normalized market fields and recovery/invalidation chronology; they do not contain connection credentials or raw account payloads. Each collector restart creates a new file and preserves prior sessions.

This feed is best effort. Successful REST recovery establishes book state, not a complete historical trade tape. Any later paper experiment must preserve local receipt chronology, subscription epochs, gaps and invalidations. Quote touches or price crossings alone are not fill evidence. Frozen historical paper cohorts keep their original journals, parameters and source identities.

Contract sources: the official SIG API guide and OpenAPI Realtime tag, checked 2026-10-02, plus the pinned official `@supabase/supabase-js` client. Existing `feed/` modules are reused without changing their frozen contracts.

## Passive opportunity observer

`Start-Passive.ps1 -Initialize` freezes a new measurement cohort against the current realtime capture session, source prefix, code identities and future cutoff. It refuses to overwrite an existing bank or journal. Run the launcher without `-Initialize` to replay and resume the same cohort. `PASSIVE_STOP` stops this observer alone. A different realtime session ends the observer; preserve its artifacts before starting a separately named successor.

The observer makes no API calls and no orders. It proposes two distinct hypothetical prices one tick inside a sufficiently wide spread, waits for a later independent nonmarketable activation observation, then records strictly later trade-through evidence. It preserves the highest observed same-or-better queue quantity and does not consume that queue using cancellations or observed trades. Neither a touched quote nor a trade-through is labeled a verified fill. Fixed later price observations measure adverse selection, with missed observations and broken continuity explicitly censored. P&L and simulated-fill fields remain null.

`passive-stats.json` and `/api/passive` publish measurements separately from official trades and existing paper wallets. The observer's own fsynced hash-chain journal supports exact replay; a changed source prefix, processed capture row, code identity or incomplete journal fails closed. Connection success alone is not market-stream evidence: `/api/realtime` distinguishes successful subscriptions from receipt of the first market frame.
