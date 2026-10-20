[Fresh SIG-source status at04:28UTC October20](sig-source-service-metadata-20261020.json): existing VPS live worker and capture service remain inactive/MainPID0; live execution STOP and shared NETWORK_DISABLED remain present. This read-only audit changed no service/control/account state and recovered no continuous SIG history. Earlier observations below are historical.

# Existing external capture is live; chronological qualification is unchanged

At approximately04:00UTC on October20, a metadata-only audit found the existing Kalshi and Polymarket processes running with the expected collector scripts. It read allowlisted status fields and the existing archive manifest, then hashed two compressed snapshots without decoding observation bodies. No collector was started, reconfigured or contacted over HTTP.

| Existing source | Reported state | Status evidence |
| --- | --- | --- |
| Kalshi, PID9956 | collecting, read-only |20starts in the last second;1,192in the last minute;237cached mapped tickers; zero reported throttles |
| Polymarket, PID43224 | running | configured ceiling30requests/second;378cached outcomes over189markets;378WS outcomes; WS failure flag false |
| Local byte archive | copying existing originals | fresh status/manifest records; selected latest Kalshi/R57bcompressed snapshot hashes and sizes match the manifest |

These are collector-reported counters plus process/file verification, not an independent wire audit or new throughput benchmark. Polymarket's configured ceiling does not establish30actual requests every second. Ticker/outcome counts do not prove contract-resolution equivalence or complete SIG-market coverage; Polymarket pricing-authority and resolution-equivalence flags remain false.

[Exact metadata, counters, manifest-prefix hash and byte checks](external-capture-metadata-20261020.json).

External bytes are still retained while training runs, supporting future verified data admission. This does not mean the frozen learner consumes live caches. GPU020and the checked GPU021handoff retain their source/data identities.

The manifest contains Kalshi and four Polymarket capture namespaces. Its timestamps record file-copy times, not market-observation coverage, independent opportunities or eligible test days. Snapshots may overlap; file/byte counts are not distinct new history. Hashing confirms only selected immutable bytes and sizes.

No original observation body was decoded, including R57b. No protected calendar was reassigned, inspected for prices, used for gradients or relabeled independent. No data-use record or dataset seal changed. This audit recovers no new SIG source and does not establish missing pre-cutoff history, calibrated execution/valuation or the untouched four-day test. The [chronological readiness limitations](history-readiness-20261019.md) remain.

This read-only availability check leaves existing collectors, archive, trainer, queued controller, production controls and financial state unchanged. No new correctness test is claimed for a static audit: evidence consists of live process verification, status timestamps, a bounded manifest prefix and independently recomputed compressed-file SHA256 values.
