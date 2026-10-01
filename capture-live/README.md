# Bounded book transport integration — draft

This isolated package adds a supported Supabase SDK adapter to reviewed feed PR #6
(f21c1e8556a6c13fc024521cb089cdee9527124e). It does not modify feed/, paper/, ml/ or bot/.
The host injects the documented SDK `createClient`, a credential already held in memory,
and a real shared account admission authority. There is no autonomous CLI or credential
loader here: importing these modules creates no client, reads no secrets and sends no traffic.

## Offline verification

From this branch's repository root, Node.js 22+:

```sh
node --test capture-live/*.test.mjs bot/*.test.mjs
node --check capture-live/transport.mjs
node --check capture-live/pilot.mjs
```

14 transport/pilot tests plus the 11 scanner tests pass on Node 24.19.0. All injected
SDKs, tokens, books and transports in tests are invented. The deterministic fake-clock
pilot captures one synthetic exchange; that verifies orchestration only. It is not a
live connection, market-data sample, calibrated fill model or evidence of profitable ML.
No third-party package is imported by the testable core; a future host must pin the
supported @supabase/supabase-js SDK version and dependency lockfile before deployment.

## Host interface

`new BookTransport({scope, key, createClient, budget, tokenSetupApproved, ...})` requires:

- `scope`: exact tournament UUID, market ID and all configured exchange IDs, validated
  with feed/contract.mjs. Only that private tournament market topic is allowed.
- `key`: existing approved API credential in memory. It is a private class field;
  never put it in config, JSON, command-line arguments, manifests or logs.
- `createClient`: documented Supabase SDK factory. Auth persistence, automatic session
  refresh and URL session detection are disabled. No browser-session extraction.
- `budget`: `{sharedAuthority: true, admit, cooldown}` from a real account-wide service.
  Every request passes `{method, path, cost: 1, signal}` to admission. Cooldowns pass
  finite milliseconds and a manual-review sentinel for excessive Retry-After.
  The flag is an interface assertion, not a distributed implementation or proof.
- `tokenSetupApproved: true`: only after actual separate authorization/review required
  by AGENTS.md and feed/README.md. A software flag cannot grant human authorization.

The only REST POST allowed is `/realtime/token`, which costs one write request and
creates an ephemeral three-hour realtime credential. GET allows only the configured
market's tournament orderbook. No order placement, cancellation or account subscription
exists. SDK connection parameters come only from successful official token bootstrap;
HTTPS is mandatory and refresh may not change origin. The SIG key goes only to the
fixed official SIG API host; redirects are rejected. No bootstrap response is captured.

`capturePilot({scope, transport, capture, signal, shouldStop, ...})` runs at most 60 seconds,
two connections and 8 recoveries by default (hard cap 10). It buffers at most 1,000
pre-ack frames preserving callback receive time, performs mandatory initial/restoration
REST reads, ticks freshness, reconnects on SDK failure, and reauthenticates near expiry.
Recovery is spaced by at least two seconds and remains subject to shared admission.
Unknown/malformed snapshot state remains unready. Settlement invalidation blocks the
pilot pending separate authoritative market-state reconciliation. Errors stop safely
and the summary is limited to counters/state; raw SDK errors and auth are not logged.
STOP is checked before any transport/capture call, queued waits are interruptible,
and a hard deadline bounds uncooperative subscription/recovery calls and cleanup.
The caller must check STOP before credential loading or opening the Capture sink too.

Inject the reviewed exclusive `Capture` sink from feed/capture.mjs. Capture only normalized
books/lifecycle records, outside Git in a private ignored directory. The final summary
records the observation window, request attempts, recovery/connection counts and coverage.
Configured coverage is not all-universe coverage; tradeTapeComplete and execution
eligibility remain false. No unattributed trade becomes a paper fill. No P&L is produced.

## Unfinished live pilot gates

No token was minted, existing credential loaded, SDK installed, authenticated request
sent or real market capture performed for this revision. A service-backed shared GET/POST
budget with cooldown propagation has not been provided or deployed. Token setup is
awaiting the separate review/authorization in the current repository instructions.
There is no committed real-data fixture, latency benchmark or completeness claim.

The integration is reviewable code with offline tests, not a finished operating host.
Before pilot: provide/review the SDK host and pinned runtime, shared account authority,
approved ephemeral-token setup, fresh read-only account/market reconciliation and explicit
small request reservation agreed with the sole executor. Preserve concurrent order ownership;
a capture pilot must not touch orders. Publish the actual outcome and sanitized provenance
manifest after a separately gated run, including gaps, readiness intervals, observation
latency and failures rather than asserting a connected client means complete history.

## Sources

[Official SIG API v1 reference](https://sig.thesuper.market/api/v1/docs), Realtime token,
private Supabase broadcasts and tournament orderbook contract; locally reviewed official
reference retrieved October 1, 2026, plus feed/CONTRACT.md and feed/SCHEMA.md at f21c1e8.
The web reader could not load the reference during this revision; the existing same-day
public API specification/guide and exact reviewed contract were used. Recheck the public
reference before live integration. No undocumented socket protocol was implemented.

