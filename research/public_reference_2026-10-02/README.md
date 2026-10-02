# Public election reference · 2026-10-02

Compact, public-only research bundle for the simulated Predictions Cup. It contains **993 generic-party contracts across 496 races**: 427 House districts, 35 Senate races, and 34 governor races. Party counts are 496 Democratic, 496 Republican, and 1 Independent. Fourteen candidate-specific/nonstandard contracts remain separate. It is broad partial coverage, not a complete market catalog.

## Read the data

From this directory, run the standard-library offline verifier:

```sh
python verify_bundle.py
```

Read directly in Python without creating another file:

```python
import gzip, json
with gzip.open("reference.json.gz", "rt", encoding="utf-8") as f:
    reference = json.load(f)
assert len(reference["markets"]) == 993
```

Or decompress with `gzip -dc reference.json.gz > reference.json` (PowerShell users should use Python binary I/O rather than text redirection). The gzip uses compression level 9, mtime 0 and an empty filename field. Original reference bytes are preserved exactly.

- Compressed: 129,544 bytes; SHA-256 `b4c4c2cf6cfc1de7b5e505356422d741de49a81aee230f4880fda1ebe195b034`
- Uncompressed: 3,730,681 bytes; SHA-256 `bfa0d2095071986841a2ef302385a7565f0574c4d13a2b909edb6eec6bb1cb1b`

## Files and provenance

- `reference.json.gz`: normalized race/party identity, raw titles/rules, direct reported YES/NO bid/ask fields, YES top sizes, receipt times, contract/source URLs and uncertainty flags
- `coverage_report.md` / `coverage.json`: coverage, discovery method, gaps and limitations
- `orderbook_snapshots.json` / `orderbook_tops.csv`: six later full books; direct bids and explicitly derived complementary asks
- `raw/*_orderbook_*.json`: the six exact raw public order-book response bodies, retaining the paths referenced by the normalized records
- `candidate_or_nonstandard_markets.json`, `candidate_event_ids.json`, `excluded_events.json`: separately classified contracts and discovery/exclusion evidence
- `request_log.jsonl`: exact 34 public API request URLs, local sent/receipt UTC, status and non-authentication response headers; six documentation page retrievals were additionally reported in the original research
- `deliverable_manifest.json` / `qa_summary.txt`: original research validation artifacts, copied unchanged; the original manifest also lists files intentionally omitted here
- `source_evidence_manifest.json`: sizes and SHA-256 for every original non-bytecode file, publication mapping, exact raw-response URLs and explicitly omitted evidence
- `publication_validation.json`: offline packaging/count/hash validation
- `bundle_manifest.json`: hashes of every published file other than itself
- `source_scripts/*.py`: exact collection/normalization/finalization source, retained for audit only. These scripts were not run during publication. The collection scripts contain network reads and should not be launched as part of offline verification. They originally expected their data beside the scripts, not the `source_scripts/` layout

## Intentional omissions and reproducibility boundary

The 18.5 MB unfiltered raw series catalog, raw event response bodies, selected-series catalog, observed-events intermediate, and all-returned-event metadata are **not included** in this compact commit. The duplicate 1.8 MB `reference.csv` is omitted; its original hash is retained. These originals remain in the source capture outside this Git bundle. `source_evidence_manifest.json` explicitly identifies each omitted file with its exact byte size and SHA-256; raw-response URLs and times are retained there and in the request log.

Therefore this bundle supports byte verification and analysis of the published derived reference and the six published raw books, **not full raw-to-normalized reproducibility or independent authentication of every source response**. A later fetch from the same URL can return changed data. Mentions of `raw/` and `reference.csv` in the unchanged original report/manifests describe the original capture, not the complete contents of this publication. No secrets, account data, credentials, cookies, or unrelated personal files are included.

## Time and interpretation

Main quotes were received **2026-10-02 05:15:54.615017–05:20:00.384744 UTC**. Six books were received **05:20:37.714763–05:21:16.977518 UTC**. Receipt time is not exchange quote creation time; source quote age is unknown for all records. Metadata update times are not quote times. The books are not synchronized with the main table. NO-side event-market sizes are unknown; zero YES size is not executable liquidity.

These are mixed-time lead-generation observations, not a fair-probability estimate, guaranteed arbitrage, current execution table or probability-range input ready for a paper cohort. Before joining to a Cup contract, verify exact race, year, party versus candidate, special-election status, full settlement predicate, inauguration/sworn-winner rules, independent treatment, fees and depth. Do not treat distinct party contracts as automatic complements.

Official sources: [series](https://docs.kalshi.com/api-reference/market/get-series-list), [events](https://docs.kalshi.com/api-reference/events/get-events), [market fields](https://docs.kalshi.com/api-reference/market/get-markets), [order books](https://docs.kalshi.com/api-reference/market/get-market-orderbook). This publication makes no new source requests and changes no trading or collection state.
