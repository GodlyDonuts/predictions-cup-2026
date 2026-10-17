# Live public Polymarket reference feed

The separate public-data service is live on the production host as PID691529 since **2026-10-17 18:55:43 UTC**, using source `8abbed2e98f11b0b132d6fecbd6fc9cdcbba10d9`. It creates no SIG account client and uses no Polymarket credentials or trading routes. The SIG worker is separate and is still restoring at the latest checked observation.

The **268.875-second measurement ending19:02:03 UTC** observed29.538 admitted and completed requests/second,59.080 successful outcome-book updates/second,378 cached outcome books and189 fully covered binary markets. At the endpoint, receipt age was3.165seconds median/6.294seconds maximum. Source-book timestamp age was14.382seconds median/54.835seconds maximum; a newly received unchanged book does not become a newly published source observation. HTTP errors and throttles were zero in this capture.

The public market stream had one connection,40,119 received frames,378 reconstructed outcome books and zero reconnects, invalid frames, dropped frames or queued frames at that cutoff. This is a measured interval, not a future uptime guarantee. Raw public responses and source/request/receipt clocks are retained. Original wire frames are stored once with a real connection/frame ordinal; derived per-asset cache files reference them and coalesce writes every200ms. Disconnects invalidate stream books, and HTTP views apply the original source/receipt time gates.

The first service launch exposed an actual sink bottleneck: a large array frame was redundantly captured for each outcome. It achieved26.321requests/second over300seconds with streaming pressure gaps. Those gaps remain explicit in the original evidence. The second release removes repeated frame storage and whole-market state scans, batches bounded journal compression/appends, and coalesces derived writes. The new measurements above do not retroactively repair the first launch's missing stream coverage.

## Coverage and limits

One-off Gamma discovery captured117 actual responses with zero HTTP errors:88 active events,189 winner questions and378 outcome IDs. Conditional race references cover162 of237 SIG identities;158 have same-party contextual references and75 remain unmatched. Candidate, party and opposite-outcome predicates are kept separate. This catalog is a bounded discovery pass, not the entire Polymarket universe or verified settlement equivalence.

The collector targets30 public requests/second with a rolling ceiling, smooth pacing,12 bounded concurrent requests and batches of up to two outcome books. Actual request throughput, book updates and complete-market coverage are distinct counters. Published [Polymarket limits](https://docs.polymarket.com/api-reference/rate-limits) currently allow500requests/10seconds for `/books`; the documented [batch endpoint](https://docs.polymarket.com/market-data/prices-order-books) is a read-only POST. SIG's separate account limits and Kalshi's token budget remain unchanged.

The public [market stream](https://docs.polymarket.com/market-data/realtime-data) supplies snapshots, price changes and trade information. Original outcome IDs are used: legacy token IDs or the documented V2 position IDs. No IDs or resolver equivalence are invented from names. These observations are not calibrated probabilities or executable SIG depth.

## Operation

```text
node moloch/external-data/polymarket-universe.mjs --output ABSOLUTE_UNIVERSE_JSON --repository-root ABSOLUTE_REPOSITORY --cache PRIVATE_CATALOG_DIRECTORY
node moloch/external-data/polymarket-live.mjs --cache PRIVATE_CACHE_DIRECTORY --universe ABSOLUTE_UNIVERSE_JSON --rps 30 --concurrency 12 --batch-size 2 --port 17842 --ws-port 17843
```

Both HTTP services bind127.0.0.1. REST17842 exposes `/status`, `/quotes` and `/book?assetId=...`; WS17843 exposes time-gated `/status`, `/quotes` and `/book?assetId=...`. The private production user service is `moloch-polymarket-public.service`, with restart handling, an exclusive collector lock, persistent Retry-After and bounded journal retention. A cache `STOP` requests a graceful public-feed shutdown. Research consumes the original cache without contacting SIG again.

All71 combined tests passed locally and on the actual ARM64 host. The217-member second release bundle SHA256 is `92ad87016c1c1aa46ca1dff36c7adf30d1103543b280f38f7398081ecbc0bad7`; its actual universe SHA256 is `6fc2e7f4b53ebd300d97d3bd74faa505b246156b30b320c99ba8f201ad2889a5`. The private Node24.22 runtime came from the official checksum-verified ARM64 archive; ws8.22 matches the committed lockfile integrity. No OS-wide runtime change or SIG restart was required for this feed.

The output has `pricingAuthority:false` and `resolutionEquivalenceVerified:false`. Current Rust production valuation does not yet use Polymarket as an activated forecast. Integration must establish the actual contract predicates and incremental executable value; merely receiving another market price establishes neither. [Task172](https://github.com/GodlyDonuts/predictions-cup-2026/issues/172).
