# Disabled ARM64 VPS deployment

The release supports Linux without PowerShell or Windows DPAPI. Node runs the
I/O and accounting components; `moloch/native/target/release/moloch-replay` is
compiled natively for accelerated offline replay. At 100 HTTP requests/minute,
the collector needs about 1.67 requests/second, not an HFT matching engine.

The portable collector has a **100 read admissions/minute** rolling cap, 600ms
minimum start spacing, one serialized client, persistent Retry-After/backoff,
and a separate minimum 60-second authentication interval. Actual throughput can
be lower under network delay, endpoint caching, or server backoff. Writes are
restricted to one-shot realtime authentication; there is no order endpoint.
Use multiple markets: repeated order-book reads inside one second add no fresh
information, and other REST market data can be cached for two seconds.

Deployment remains inert: `deployment.json` disables network and trading;
`runtime.env` sets `MOLOCH_NETWORK_ENABLED=0`; both `NETWORK_DISABLED` and
`EXECUTION_STOP` exist. No SIG key is transferred and no bot service is enabled.
Readiness verification and tests make no SIG API requests.

The service file is a **template**, not an activation instruction. There is no
`NETWORK_ENABLED` marker. Even a mistaken service start will not pass its
condition. The collector separately checks its deployment configuration,
environment and stop marker before loading credentials or opening a lock.

All keys and hosts share the same account budget. Before any later human-
authorized activation, stop/reconcile existing collectors and establish account
ownership and one account-wide limiter. A local PID lock does not enforce
cross-host exclusivity. Do not run this allocation alongside Baal's read budget.

This installation contains code, locked realtime dependencies, an offline replay
fixture and a closed-wallet result summary. Active Windows journals and API
credentials are not copied. A later financial-state migration must preserve and
verify the complete original histories and resolve Windows paths; do not reset
the old wallet or silently treat the default simulator balance as transferred
capital. The current verified paper result is 99,849.720 points and -150.280
cumulative realized P&L for the strict wallet. This is not contest-account cash.
