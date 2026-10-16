# Kalshi reference data for Moloch

The continuous collector uses the existing local read-only Ed25519 key. It targets20 total authenticated GETs/second with a single shared200-token/second admission bucket, a20-request rolling ceiling, bounded concurrency, actual endpoint costs and persisted Retry-After. Market, initialization and mapping-discovery reads share this budget. It places no Kalshi orders and creates no SIG client.

The latest universe provides **234 direct party references plus three separate candidate analogs**, covering all237 SIG identities with external information. The three independent-party identities191/276/280 have candidate-specific references with explicit rule mismatches; they remain excluded from primary party mappings. All mappings are conditional, and none grants pricing or execution authority. See the [source-pinned mapping evidence](../../research/moloch-kalshi-mapping-20261016/README.md).

At20 reads/second,237 one-book polls have an ideal11.85-second rotation. Network latency, discovery reads, errors and backoff add delay. Initial metadata must be validated; subsequent restarts may reuse original verified metadata within its six-hour validity and original cached books without renewing their timestamps. Report actual refresh ages and throughput, not just configured limits.

```text
node moloch/external-data/kalshi-collector.mjs --env-file ABSOLUTE_IGNORED_ENV_PATH --cache PRIVATE_CACHE_DIRECTORY --repository-root REPOSITORY_DIRECTORY --port 17841
```

Use the existing ignored `.env` described in [Kalshi access documentation](../../docs/KALSHI_MARKET_DATA.md). Keys, signatures and request headers are excluded from receipts, caches and logs. The collector owns `collector.lock`; it refuses live/corrupt ownership and serializes positively dead-owner reclamation. It does not create credentials or copy them to another host.

Cache layout:

- `latest/TICKER.json`: atomic original response and exact normalized bounded depth, with request/receipt clocks and body hash.
- `metadata/TICKER.json`: original metadata receipt, including resolution rules.
- `journal/*.ndjson.gz`: original successful public responses, level1 compression; retain72hours/approximately2GiB plus the current bounded64MiB segment. Active gzip segments finalize on rotation or graceful stop.
- `status.json`, `universe.json`, `admission-profile.json`, `backoff.json`: coverage, actual rates, ages, limits, costs and restart backoff.

The server binds **127.0.0.1 only**:

| Local route | Contents |
| --- | --- |
| `GET /v1/status` | Running PID, actual starts, errors, coverage and receipt ages |
| `GET /v1/quotes` | All current normalized books and mapping classifications |
| `GET /v1/book/TICKER` | Original bounded book receipt and exact normalized levels |
| `GET /v1/discover?path=ENCODED_PUBLIC_API_PATH` | One catalog discovery read through the same account limiter; busy requests are declined |

A private SSH reverse forward makes this loopback cache available on the production VPS without transferring the key. It depends on the Windows collector and SSH connection staying available. Per-record receipt clocks expose stale data if either fails. The SIG worker continues independently; **Kalshi prices are not yet an activated production valuation model**. Book prices are external quotes, not guaranteed probabilities or executable SIG depth.

Create a `STOP` file in the cache directory for graceful shutdown. Queued requests retain their admission/backoff constraints; stop is cooperative. No retry creates an independent rate allowance. Run offline checks with `node --test moloch/external-data/*.test.mjs`.
