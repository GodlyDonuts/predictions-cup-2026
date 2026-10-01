# Read-only feed collector scaffold

**Offline and disabled by default. No live connection is implemented or verified.** This isolated `feed/` package validates the supported tournament market-feed contract and exercises recovery with injected offline adapters. It does not read credentials or environment secrets, mint/refresh tokens, call authenticated endpoints, create subscriptions, place/cancel orders, start a scheduler, deploy, merge, or establish an ML/trading edge. Node.js 22+; zero dependencies.

## Run from repository root

```sh
node --test feed/test/*.test.mjs
node feed/cli.mjs
node feed/cli.mjs --replay feed/fixtures/scenario.jsonl
node feed/cli.mjs --replay feed/fixtures/scenario.jsonl --capture /tmp/new-feed-capture.jsonl
# From feed/: npm test ; npm run replay
```

No arguments print an off-state help message. `--live`, `--collect`, unknown flags and implicit collection are rejected. A `STOP` file in the working directory prevents even reading the input or creating output. SIGINT/SIGTERM and STOP checks interrupt replay cooperatively; the injected runtime supports interruptible admission, timeout, backoff and bounded cleanup. Captures use exclusive new files (0600), append/fsync each row and never overwrite previous captures. A crash can leave a partial final row: reject it rather than fabricate completion. Keep runtime captures outside Git or in ignored `feed/captures/`.

The committed scenario/golden result are wholly synthetic, deliberately using a synthetic tournament UUID. They are public-contract-shaped test data, not observed market history, fill calibration, training data, or performance evidence.

## Components

- `contract.mjs`: explicit normalized allowlist, context/ID/array/level checks, strict timestamps, nanosecond-precision book-version comparison and full configured exchange coverage
- `collector.mjs`: one configured tournament market, topic revision distinct from per-exchange engine book versions, duplicate/out-of-order handling, mandatory initial/reconnect/refresh/gap recovery, expiry/staleness invalidation and capture hooks
- `runtime.mjs`: disabled transport plus bounded offline subscription/recovery orchestration. Inject one shared `AccountReadBudget` callback; no process-local limiter is presented as account-wide protection
- `capture.mjs`: normalized records only, known sensitive-name and token-pattern defense-in-depth redaction, exclusive timestamped append-only JSONL
- `cli.mjs`: cooperative offline fixture replay, with optional normalized observation capture
- `CONTRACT.md` and `SCHEMA.md`: verified source semantics, adapter contract, output fields and paper/ML integration boundaries

## Safety and completeness

`booksReady` means all configured exchanges have versioned books recovered in the current subscription epoch under this local freshness policy. It is **not execution authorization**, proof of complete history, complete tournament coverage, correct fees, own-order removal, or calibrated fills. Both `tradeTapeComplete` and `universeComplete` remain false. No order interface exists.

Every new connection/refresh requires an authoritative snapshot even if stored book versions remain newer. Invalidations received during an in-flight recovery invalidate its generation. A monotonic arrival ordinal distinguishes a newer push during recovery from a pre-recovery book received in the same millisecond. Older snapshots cannot revalidate stale pre-recovery state. Unversioned REST snapshots fail closed. A settlement/refund invalidation stays blocked because a book read alone cannot establish authoritative market state; a future adapter must separately implement and review that reconciliation.

Freshness is a conservative policy (default 10 seconds), not an assertion that silent markets have changed. A future service should tune it against the account's recovery budget and coverage. REST books currently do not promise `nextExpiryAt`; a null/absent expiry is marked unknown, and timed freshness remains necessary. A pushed book with an expiry already due invalidates before any trade observation is marked continuous. Duplicate equal books do not renew freshness; authoritative equal-version snapshots do.

Trade-ID deduplication is bounded (default 100,000 IDs). Evictions are reported; old replays beyond that window cannot be treated as globally deduplicated. Dataset consumers must deduplicate across persisted captures using source identity, retain gaps and provenance, and never count both `event.payload.trades` and derived `trade_observed` rows. No unknown raw payloads, auth headers, tokens or adapter exception messages are logged.

## Live integration gate

The official contract documents Supabase private broadcasts and tournament auth via `POST /realtime/token` and `setAuth`. Token creation is a REST write and persistent/access handling requires separate review/authorization. This package does not perform it. `DisabledTransport` fails closed; `OfflineRuntime` only accepts adapters explicitly marked `mode: 'offline'`. Merely supplying an environment variable cannot activate it.

A later reviewed integration must use supported v1 resources and a documented Supabase client, not guessed socket URLs, site-internal endpoints, extracted browser credentials or undocumented auth. All API clients/keys/processes must use one service-backed account admission authority and cooldown propagation. This callback is an interface, not a deployed cross-process limiter; current `bot/` has its own limitation and is not silently integrated here. Every REST retry must reacquire admission. Retry-After is never shortened: excessive cooldown stops this bounded run. There is no token/order write callback.

`OfflineRuntime.connect()` subscribes, buffers at most 1,000 pre-ack frames, then performs REST recovery. `recover()` is explicitly invoked after later collector invalidation; there is no perpetual background loop. `tick()` must be scheduled by any future host. Adapters must honor AbortSignal, cancel underlying work, and release resources on close; the scaffold also bounds caller waits if an adapter misbehaves. A disabled/scaffold milestone must not be advertised as live collection.

Before live use: authorize token/runtime setup, implement the shared limiter service, verify exact tournament/exchange mapping, bounded subscription lifecycle/refresh, scoped REST context, settlement and trade-tape recovery, completeness, reconnect behavior and rate usage against real read-only observations. Keep trading absent. Dot retains execution ownership; PR review is not an execution handoff.
