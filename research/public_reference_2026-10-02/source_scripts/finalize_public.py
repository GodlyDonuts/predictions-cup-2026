import json,pathlib,csv,collections,datetime,decimal
P=pathlib.Path(__file__).parent; ref=json.loads((P/'reference.json').read_text());rs=ref['markets'];by={r['ticker']:r for r in rs};logs=[json.loads(x) for x in (P/'request_log.jsonl').read_text().splitlines()];ss=json.loads((P/'eligible_series_catalog.json').read_text());events=json.loads((P/'observed_events.json').read_text());books=[]
for l in logs:
 if '_orderbook_' not in l.get('file',''):continue
 ticker=l['file'].split('_orderbook_',1)[1].removesuffix('.json');d=json.loads((P/l['file']).read_text())['orderbook_fp'];yes=d.get('yes_dollars') or [];no=d.get('no_dollars') or [];yb=max(yes,key=lambda x:decimal.Decimal(x[0])) if yes else None;nb=max(no,key=lambda x:decimal.Decimal(x[0])) if no else None
 n=decimal.Decimal(by[ticker]['notional_value_dollars']);fmt=lambda x:format(x,'.4f')
 b={'ticker':ticker,'receipt_utc':l['receipt_utc'],'source_url':l['url'],'source_quote_timestamp':None,'source_quote_age_seconds':None,'source_quote_age_status':'unknown_not_provided','yes_bid_dollars':yb[0] if yb else None,'yes_bid_size_fp':yb[1] if yb else None,'no_bid_dollars':nb[0] if nb else None,'no_bid_size_fp':nb[1] if nb else None,'implied_yes_ask_dollars':fmt(n-decimal.Decimal(nb[0])) if nb else None,'implied_yes_ask_size_fp':nb[1] if nb else None,'implied_no_ask_dollars':fmt(n-decimal.Decimal(yb[0])) if yb else None,'implied_no_ask_size_fp':yb[1] if yb else None,'ask_derivation':'notional minus opposite-side best bid; computed from bid book, not separately reported ask','yes_bid_levels':len(yes),'no_bid_levels':len(no),'raw_file':l['file'],'orderbook_fp':d}
 books.append(b)
(P/'orderbook_snapshots.json').write_text(json.dumps({'note':'Six separately timed full anonymous book snapshots; source quote age is unknown. Bid prices and sizes are direct observations; complementary asks are explicitly derived.', 'books':books},indent=2)+'\n')
with (P/'orderbook_tops.csv').open('w',newline='') as f:
 fields=[k for k in books[0] if k!='orderbook_fp'];w=csv.DictWriter(f,fieldnames=fields);w.writeheader();w.writerows({k:b[k] for k in fields} for b in books)
unmatched=[s['ticker'] for s in ss if not s['ticker'].startswith('KX') and s['ticker'] not in {e['series_ticker'] for e in events}]
all_returned_events=[]
for l in logs:
 if l.get('status')==200 and l.get('file'):
  all_returned_events.extend(json.loads((P/l['file']).read_text()).get('events',[]))
empty=[{'event_ticker':e['event_ticker'],'title':e['title'],'year_verification':'explicit_2026' if '2026' in e.get('sub_title','') else '26_suffix_only_no_markets'} for e in all_returned_events if not e.get('markets')]
(P/'all_returned_event_metadata.json').write_text(json.dumps([{k:v for k,v in e.items() if k!='markets'} for e in all_returned_events],indent=2)+'\n')
coverage={'generic_party_markets':len(rs),'distinct_races_with_generic_party_markets':len(set(r['race_key'] for r in rs)),'market_counts_by_office':dict(collections.Counter(r['office'] for r in rs)),'race_counts_by_office':{o:len(set(r['race_key'] for r in rs if r['office']==o)) for o in ['House','Senate','Governor']},'party_counts':dict(collections.Counter(r['party'] for r in rs)),'market_statuses':dict(collections.Counter(r['status'] for r in rs)),'all_series_catalog_size':len(json.loads((P/'raw/001_series.json').read_text())['series']),'eligible_contract_terms_series_count':len(ss),'legacy_candidate_event_ids_queried':len(json.loads((P/'candidate_event_ids.json').read_text())['candidates']),'legacy_candidate_event_ids_found':sum(1 for e in events if not e['series_ticker'].startswith('KX')),'legacy_series_no_conventional_2026_event_found':unmatched,'kx_series_enumerated':sum(s['ticker'].startswith('KX') for s in ss),'kxhouserace_pages':2,'kxhouserace_events':sum(e['series_ticker']=='KXHOUSERACE' for e in all_returned_events),'all_returned_events':len(all_returned_events),'events_returned_with_no_nested_market':empty,'named_candidate_or_nonstandard_markets_separate':ref['metadata']['excluded_candidate_or_nonstandard_market_count'],'top_yes_price_and_size_missing':sum(any(r[k] is None for k in ['yes_bid_dollars','yes_ask_dollars','yes_bid_size_fp','yes_ask_size_fp']) for r in rs),'zero_yes_bid_size_count':sum(decimal.Decimal(r['yes_bid_size_fp'])==0 for r in rs),'zero_yes_ask_size_count':sum(decimal.Decimal(r['yes_ask_size_fp'])==0 for r in rs),'no_side_top_size_unknown_count':sum(r['no_bid_size_fp'] is None or r['no_ask_size_fp'] is None for r in rs),'source_quote_age_unknown_count':len(rs),'api_request_count':len(logs),'documentation_page_retrievals':6,'total_public_requests':len(logs)+6,'status_codes':dict(collections.Counter(str(l['status']) for l in logs)),'full_book_count':len(books),'public_api_collection_start_utc':logs[0]['sent_utc'],'public_api_collection_end_utc':logs[-1]['receipt_utc'],'top_market_quote_receipt_start_utc':ref['metadata']['receipt_start_utc'],'top_market_quote_receipt_end_utc':ref['metadata']['receipt_end_utc'],'orderbook_receipt_start_utc':min(b['receipt_utc'] for b in books),'orderbook_receipt_end_utc':max(b['receipt_utc'] for b in books),'exhaustive_coverage_claim':False}
(P/'coverage.json').write_text(json.dumps(coverage,indent=2)+'\n')
text=f'''# Public 2026 election-party reference

Built from anonymous official Kalshi public APIs for free simulated Cup lead generation. No credentials, account reads, real-money trades, order endpoints, or Cup collection were used.

## Result and files
- {coverage['generic_party_markets']} generic-party contracts across {coverage['distinct_races_with_generic_party_markets']} distinct race events: {coverage['race_counts_by_office']['House']} House districts, {coverage['race_counts_by_office']['Senate']} Senate races, and {coverage['race_counts_by_office']['Governor']} governor races
- Party counts: {coverage['party_counts']}
- All included market statuses were active at receipt
- reference.json and reference.csv contain normalized race/party identity, raw title and rules, direct YES/NO bid/ask prices, YES top sizes, receipt UTC, metadata update times, terms/certification/source links, and data-quality flags
- orderbook_snapshots.json retains six full books; orderbook_tops.csv has direct bids and explicitly derived complementary asks, each with its own later receipt UTC
- candidate_or_nonstandard_markets.json preserves {coverage['named_candidate_or_nonstandard_markets_separate']} separately excluded named-candidate/nonstandard-party contracts. Never join one to a generic independent-party outcome just by its UI label
- coverage.json contains machine-readable coverage and gaps. raw/ and request_log.jsonl retain the public source evidence

## Timing and quote limits
- Event-market quotes received {coverage['top_market_quote_receipt_start_utc']} through {coverage['top_market_quote_receipt_end_utc']}
- Six full books received {coverage['orderbook_receipt_start_utc']} through {coverage['orderbook_receipt_end_utc']}
- Source quote timestamp/age is unknown for every record. Receipt time is local response receipt, not quote creation time
- market_metadata_updated_time is non-trading metadata time, NOT quote age. HTTP Date is retained separately
- All main-table YES price and size fields are present; {coverage['zero_yes_bid_size_count']} have zero YES bid size. Zero size is not executable liquidity
- NO prices are directly reported, but NO sizes are absent from event-market responses and remain unknown. The six separate books add direct bid-side depth and sizes
- Main-table depth is only the reported top YES sizes except where a separate full-book file is linked. Full-book snapshots are not synchronized with the event quotes

## Discovery and actual coverage
- One unfiltered documented /series response returned {coverage['all_series_catalog_size']} series. A local exact contract-terms-file filter selected {coverage['eligible_contract_terms_series_count']} HOUSEPARTY/SENATEPARTY/GOVPARTY series
- The documented /events tickers filter validated {coverage['legacy_candidate_event_ids_found']} conventional -26 event IDs out of {coverage['legacy_candidate_event_ids_queried']} catalog-derived candidates in five batches. Candidate IDs were a discovery heuristic, not accepted data; only returned events entered the reference
- All {coverage['kx_series_enumerated']} selected KX-prefixed series were explicitly enumerated via /events?series_ticker= with nested markets. KXHOUSERACE yielded {coverage['kxhouserace_events']} events through two cursor pages; all returned cursors were exhausted
- Twelve returned events have no nested markets (four explicitly labeled 2026; eight CA KXHOUSERACE events only have a -26 suffix and cannot independently establish year from absent market rules): {', '.join(e['event_ticker'] for e in empty)}
- This is broad partial coverage, not a complete catalog claim. {len(unmatched)} legacy series had no conventional -26 event returned; older-year, differently named, archived, or other-contract-template events were not exhaustively searched. No sports or 2028 markets were included
- {coverage['api_request_count']} anonymous API GETs plus six documentation page retrievals = {coverage['total_public_requests']} total public requests. All API GETs returned 200. Requests were serial with at least three seconds between API response and next request (except the first request); no 403/429 bypass or retries occurred

## Matching cautions
This is a lead-generation reference, not a fair-probability estimate or an arbitrage claim. Race/party name agreement is only a proposed join. Compare full rules: sworn-in/inaugurated winner, term/year, special election, party versus named candidate, independent treatment, accelerated determination, resolution timing, fees, and actual available depth. Distinct Democratic/Republican contracts should not automatically be treated as exact complements. Florida and Ohio Senate entries are explicitly special-election race keys. Minnesota Democratic (DFL) rules are preserved verbatim.

## Official primary sources
- Series/category discovery and terms fields: https://docs.kalshi.com/api-reference/market/get-series-list
- Event batch filtering, series filtering, nested markets, cursor pagination: https://docs.kalshi.com/api-reference/events/get-events
- Market quote fields and metadata-updated semantics: https://docs.kalshi.com/api-reference/market/get-markets
- Public order books: https://docs.kalshi.com/api-reference/market/get-market-orderbook
- Exact API request URLs are in request_log.jsonl; per-record contract_terms_url comes directly from the official series catalog
'''
(P/'coverage_report.md').write_text(text)
print(json.dumps(coverage,indent=2));print('\nBOOK TOPS\n',json.dumps([{k:b[k] for k in ['ticker','receipt_utc','yes_bid_dollars','yes_bid_size_fp','implied_yes_ask_dollars','implied_yes_ask_size_fp']} for b in books],indent=2))
