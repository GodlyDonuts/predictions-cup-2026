# Realtime failure diagnosis

Previous direct and aggregate subscriptions failed with `CHANNEL_ERROR / unclassified`. That is not evidence of an authorization failure, a capacity limit, or a specific network fault. The installed official Supabase client can wrap underlying errors, so inspecting only the outer message or code loses useful evidence.

The classifier now examines at most12 unique error nodes through cause, error, reason and a bounded aggregate-error list. It stores only an allowlisted error shape, inspected-node count, category, known network error codes, valid numeric WebSocket close codes and error/close event types. It never writes raw messages, arbitrary codes, close reasons, URLs, headers or tokens. Tests cover nested DNS errors, close events, cycles, oversized chains and secret-bearing strings. Authentication and capacity categories remain message-based diagnostic inferences, not proof of the server's cause.

This change does not restart a connection, refresh credentials, increase read budgets, create an account client, or submit orders. A later bounded diagnostic attempt must wait for the existing collector to terminate and preserve every funded paper quote and position. A successful subscription alone does not prove fresh data, a usable strategy, or profit.

## Verified transport recovery

The supported Supabase transport hook now uses pinned `ws8.22.0`. Its wrapper records only numeric handshake/close status and the sanitized typed-error classifier. It refuses redirects and terminates a rejected handshake without inspecting the response body. Local socket tests exercise successful upgrade and message delivery, HTTP403 rejection and refused connections. All144 core tests pass. See the [ws API](https://github.com/websockets/ws/blob/master/doc/ws.md) for its public transport events.

Phase60 completed with HTTP101, nine pushed book updates, four official REST receipts, zero stream gaps/schema errors and one temporary auth write. No token refresh, contest orders, new observed trades or profit were established. Evidence: `data/moloch-realtime-restored-20261014-60.json`. The transport change restored actual delivery during this probe; the earlier native transport's exact failure cause remains unresolved because the connection environment may also have changed. Do not interpret successful delivery as evidence of a winning strategy.

## Typed transient DNS failure

Phase62 failed before any market receipt with `EAI_AGAIN`, now visible in both the socket and channel diagnostics. Both100,000-point banks retained their prior119 orders/five historical fills, zero owned inventory and zero unresolved quotes; the entry-improvement trial was not exercised. Evidence: `data/moloch-inside-start-failure-20261014-62.json`.

The transport now performs one configured-resolver IPv4 lookup fallback only after an ordinary lookup returns `EAI_AGAIN`. It respects an explicit IPv6-only request, retains the original hostname/SNI and default TLS verification, uses no external resolver or persistent cache, and records only started/succeeded/failed. Failed/empty fallback exits; no authentication renewal or API retry is added. Tests cover normal/permanent lookup outcomes, bounded transient resolution, invalid addresses and secret-free diagnostics. All150 core tests pass. Phase63 preregisters the same entry experiment from the verified flat failed-attempt parent; candidate and control differ only in one-tick entry improvement.
