# Public 2026 election-party reference

Built from anonymous official Kalshi public APIs for free simulated Cup lead generation. No credentials, account reads, real-money trades, order endpoints, or Cup collection were used.

## Result and files
- 993 generic-party contracts across 496 distinct race events: 427 House districts, 35 Senate races, and 34 governor races
- Party counts: {'Democratic': 496, 'Republican': 496, 'Independent': 1}
- All included market statuses were active at receipt
- reference.json and reference.csv contain normalized race/party identity, raw title and rules, direct YES/NO bid/ask prices, YES top sizes, receipt UTC, metadata update times, terms/certification/source links, and data-quality flags
- orderbook_snapshots.json retains six full books; orderbook_tops.csv has direct bids and explicitly derived complementary asks, each with its own later receipt UTC
- candidate_or_nonstandard_markets.json preserves 14 separately excluded named-candidate/nonstandard-party contracts. Never join one to a generic independent-party outcome just by its UI label
- coverage.json contains machine-readable coverage and gaps. raw/ and request_log.jsonl retain the public source evidence

## Timing and quote limits
- Event-market quotes received 2026-10-02T05:15:54.615017+00:00 through 2026-10-02T05:20:00.384744+00:00
- Six full books received 2026-10-02T05:20:37.714763+00:00 through 2026-10-02T05:21:16.977518+00:00
- Source quote timestamp/age is unknown for every record. Receipt time is local response receipt, not quote creation time
- market_metadata_updated_time is non-trading metadata time, NOT quote age. HTTP Date is retained separately
- All main-table YES price and size fields are present; 1 have zero YES bid size. Zero size is not executable liquidity
- NO prices are directly reported, but NO sizes are absent from event-market responses and remain unknown. The six separate books add direct bid-side depth and sizes
- Main-table depth is only the reported top YES sizes except where a separate full-book file is linked. Full-book snapshots are not synchronized with the event quotes

## Discovery and actual coverage
- One unfiltered documented /series response returned 14567 series. A local exact contract-terms-file filter selected 235 HOUSEPARTY/SENATEPARTY/GOVPARTY series
- The documented /events tickers filter validated 134 conventional -26 event IDs out of 214 catalog-derived candidates in five batches. Candidate IDs were a discovery heuristic, not accepted data; only returned events entered the reference
- All 21 selected KX-prefixed series were explicitly enumerated via /events?series_ticker= with nested markets. KXHOUSERACE yielded 359 events through two cursor pages; all returned cursors were exhausted
- Twelve returned events have no nested markets (four explicitly labeled 2026; eight CA KXHOUSERACE events only have a -26 suffix and cannot independently establish year from absent market rules): HOUSENH2-26, HOUSECA40-26, KXHOUSERACE-CA37-26, KXHOUSERACE-CA34-26, KXHOUSERACE-CA29-26, KXHOUSERACE-CA14-26, KXHOUSERACE-CA12-26, KXHOUSERACE-CA11-26, KXHOUSERACE-CA07-26, KXHOUSERACE-CA04-26, KXHOUSENJ11SPECIAL-26, KXHOUSENY21-26
- This is broad partial coverage, not a complete catalog claim. 80 legacy series had no conventional -26 event returned; older-year, differently named, archived, or other-contract-template events were not exhaustively searched. No sports or 2028 markets were included
- 34 anonymous API GETs plus six documentation page retrievals = 40 total public requests. All API GETs returned 200. Requests were serial with at least three seconds between API response and next request (except the first request); no 403/429 bypass or retries occurred

## Matching cautions
This is a lead-generation reference, not a fair-probability estimate or an arbitrage claim. Race/party name agreement is only a proposed join. Compare full rules: sworn-in/inaugurated winner, term/year, special election, party versus named candidate, independent treatment, accelerated determination, resolution timing, fees, and actual available depth. Distinct Democratic/Republican contracts should not automatically be treated as exact complements. Florida and Ohio Senate entries are explicitly special-election race keys. Minnesota Democratic (DFL) rules are preserved verbatim.

## Official primary sources
- Series/category discovery and terms fields: https://docs.kalshi.com/api-reference/market/get-series-list
- Event batch filtering, series filtering, nested markets, cursor pagination: https://docs.kalshi.com/api-reference/events/get-events
- Market quote fields and metadata-updated semantics: https://docs.kalshi.com/api-reference/market/get-markets
- Public order books: https://docs.kalshi.com/api-reference/market/get-market-orderbook
- Exact API request URLs are in request_log.jsonl; per-record contract_terms_url comes directly from the official series catalog
