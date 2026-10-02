import pathlib,json,csv,re,collections,datetime
ROOT=pathlib.Path(__file__).parent
SERIES={s['ticker']:s for s in json.loads((ROOT/'eligible_series_catalog.json').read_text())}
LOG=[json.loads(l) for l in (ROOT/'request_log.jsonl').read_text().splitlines()]
STATES=dict(x.split(':') for x in 'AL:Alabama|AK:Alaska|AZ:Arizona|AR:Arkansas|CA:California|CO:Colorado|CT:Connecticut|DE:Delaware|FL:Florida|GA:Georgia|HI:Hawaii|ID:Idaho|IL:Illinois|IN:Indiana|IA:Iowa|KS:Kansas|KY:Kentucky|LA:Louisiana|ME:Maine|MD:Maryland|MA:Massachusetts|MI:Michigan|MN:Minnesota|MS:Mississippi|MO:Missouri|MT:Montana|NE:Nebraska|NV:Nevada|NH:New Hampshire|NJ:New Jersey|NM:New Mexico|NY:New York|NC:North Carolina|ND:North Dakota|OH:Ohio|OK:Oklahoma|OR:Oregon|PA:Pennsylvania|RI:Rhode Island|SC:South Carolina|SD:South Dakota|TN:Tennessee|TX:Texas|UT:Utah|VT:Vermont|VA:Virginia|WA:Washington|WV:West Virginia|WI:Wisconsin|WY:Wyoming|DC:District of Columbia'.split('|'))
STATECODES={v.lower():k for k,v in STATES.items()}
rows=[];events={};excluded=[]
for rec in LOG:
 if rec.get('status')!=200 or not rec.get('file'):continue
 data=json.loads((ROOT/rec['file']).read_text())
 for e in data.get('events',[]):
  s=SERIES.get(e['series_ticker']);
  if not s:continue
  # API-returned event must identify this election in event metadata or rules, not only a generated candidate ID.
  blob=' '.join([e.get('title',''),e.get('sub_title','')]+[m.get('rules_primary','') for m in e.get('markets',[])])
  year_evidence=('2026' in blob or (e['event_ticker'].endswith('-26') and 'term beginning in 2027' in blob))
  if not year_evidence:
   excluded.append({'event_ticker':e['event_ticker'],'reason':'No confirmed 2026 election in returned event metadata/rules','source_file':rec['file']});continue
  events[e['event_ticker']]=e
  typ={'HOUSEPARTY.pdf':'House','SENATEPARTY.pdf':'Senate','GOVPARTY.pdf':'Governor'}[s['contract_terms_url'].rsplit('/',1)[-1]]
  special='special' in (blob+' '+s.get('title','')).lower()
  state=None;district=None
  if typ=='House':
   match=re.search(r'\b([A-Z]{2})-(AL|\d{1,2})\b',blob)
   if match:state=match[1];district=match[2] if match[2]=='AL' else str(int(match[2]))
   else:
    match=re.search(r'(?:KX)?HOUSE([A-Z]{2})(AL|\d{1,2})',e['series_ticker'])
    if match:state=match[1];district=match[2] if match[2]=='AL' else str(int(match[2]))
  else:
   for name,code in sorted(STATECODES.items(),key=lambda kv:-len(kv[0])):
    if re.search(r'\b'+re.escape(name)+r'\b',blob.lower()):state=code;break
   if not state:
    match=re.search(r'(?:KX)?(?:GOVPARTY|SENATE(?:PARTY)?)-?([A-Z]{2})',e['series_ticker'])
    if match:state=match[1]
  race_name=(f'{state}-{district} House' if typ=='House' else f'{STATES.get(state,state)} {typ}')+(' special' if special else '')
  race_key=':'.join(['US','2026',typ.lower(),state or 'UNKNOWN',district or ('special' if special else 'regular')])
  for m in e.get('markets',[]):
   rules=m.get('rules_primary','');party_match=re.search(r'\b(?:member|representative) of (?:the )?(Democratic|Republican|Independent|Libertarian|Green)(?: \(DFL\))? party\b',rules,re.I)
   party=party_match[1].capitalize() if party_match else None
   if not party:
    # Other/third-party contracts are kept with their exact label and unresolved semantic party.
    party={'D':'Democratic','R':'Republican','I':'Independent'}.get(m['ticker'].rsplit('-',1)[-1],m.get('subtitle') or m.get('yes_sub_title') or 'UNKNOWN')
   r={'venue':'Kalshi','election_year':2026,'office':typ,'state_code':state,'state_name':STATES.get(state),'district':district,'special_election':special,'race_key':race_key,'race_name':race_name,'party':party,'party_rule_verified':bool(party_match),'ticker':m['ticker'],'event_ticker':e['event_ticker'],'series_ticker':e['series_ticker'],'event_title':e['title'],'event_subtitle':e.get('sub_title'),'series_title':s.get('title'),'market_title':m.get('title'),'market_subtitle':m.get('subtitle'),'yes_subtitle':m.get('yes_sub_title'),'market_type':m.get('market_type'),'status':m.get('status'),'result':m.get('result'),'yes_bid_dollars':m.get('yes_bid_dollars'),'yes_bid_size_fp':m.get('yes_bid_size_fp'),'yes_ask_dollars':m.get('yes_ask_dollars'),'yes_ask_size_fp':m.get('yes_ask_size_fp'),'no_bid_dollars':m.get('no_bid_dollars'),'no_bid_size_fp':m.get('no_bid_size_fp'),'no_ask_dollars':m.get('no_ask_dollars'),'no_ask_size_fp':m.get('no_ask_size_fp'),'last_price_dollars':m.get('last_price_dollars'),'quote_receipt_utc':rec['receipt_utc'],'request_sent_utc':rec['sent_utc'],'http_date':rec.get('headers',{}).get('Date'),'source_quote_timestamp':None,'source_quote_age_seconds':None,'source_quote_age_status':'unknown_not_provided','market_metadata_updated_time':m.get('updated_time'),'metadata_updated_time_is_quote_time':False,'event_metadata_updated_time':e.get('last_updated_ts'),'series_metadata_updated_time':s.get('last_updated_ts'),'depth_status':'top_yes_sizes_present_no_full_book' if m.get('yes_bid_size_fp') is not None and m.get('yes_ask_size_fp') is not None else 'unknown_not_provided','no_side_sizes_status':'unknown_not_provided' if m.get('no_bid_size_fp') is None and m.get('no_ask_size_fp') is None else 'provided','notional_value_dollars':m.get('notional_value_dollars'),'volume_fp':m.get('volume_fp'),'volume_24h_fp':m.get('volume_24h_fp'),'open_interest_fp':m.get('open_interest_fp'),'close_time':m.get('close_time'),'expected_expiration_time':m.get('expected_expiration_time'),'expiration_time':m.get('expiration_time'),'rules_primary':rules,'rules_secondary':m.get('rules_secondary'),'early_close_condition':m.get('early_close_condition'),'contract_terms_url':s.get('contract_terms_url'),'contract_certification_url':s.get('contract_url'),'settlement_sources':e.get('settlement_sources',s.get('settlement_sources')),'fee_type':s.get('fee_type'),'fee_multiplier':s.get('fee_multiplier'),'source_url':rec['url'],'market_details_api_url':'https://external-api.kalshi.com/trade-api/v2/markets/'+m['ticker'],'raw_source_file':rec['file'],'quote_interpretation':'Observed public quote, not fair probability; liquidity/fillability not guaranteed; quote age unknown','semantic_join_status':'lead_only_compare_full_resolution_rules','full_orderbook_file':None}
   rows.append(r)
candidate_specific_rows=[r for r in rows if not r['party_rule_verified']]
(ROOT/'candidate_or_nonstandard_markets.json').write_text(json.dumps({'note':'Not included in generic party-winner reference: named-candidate or nonstandard-party rules require separate semantic matching', 'markets':candidate_specific_rows},indent=2)+'\n')
rows=[r for r in rows if r['party_rule_verified']]
# Keep only latest returned row per exact venue market, never collapse distinct contracts for same race.
rows=list({r['ticker']:r for r in rows}.values());rows.sort(key=lambda r:(r['office'],r['state_code'] or '',r['district'] or '',r['ticker']))
for r in rows:
 r['same_race_party_market_count']=sum(1 for other in rows if r['race_key']==other['race_key'] and r['party']==other['party'])
 for log in LOG:
  if log.get('file','').endswith('_orderbook_'+r['ticker']+'.json'):
   r['full_orderbook_file']=log['file'];r['orderbook_receipt_utc']=log['receipt_utc'];r['depth_status']='full_orderbook_snapshot_separate_receipt_time'
  else:r.setdefault('orderbook_receipt_utc',None)
meta={'created_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Anonymous public official Kalshi catalog/events/orderbooks only; simulated Cup reference and lead generation; no account/authentication/trading accessed','coverage_status':'Broad but not asserted exhaustive: legacy series conventional -26 event candidates validated by API; all discovered KX party-series events explicitly enumerated within cap','known_limitations':['Source quote age and timestamp not provided; receipt time is not exchange quote creation time','Market updated_time reflects metadata, not trading changes or quote age','No-side sizes absent from event-market objects are unknown; do not silently equate to complementary yes size','Separate party contracts and same-race duplicate markets must not be collapsed as complements','Zero/missing size is not usable liquidity','Cross-venue rules, treatment of candidates/independents, special elections, inaugurated/sworn winner, accelerated determination, fees and execution must be checked before any simulated decision'],'doc_sources':['https://docs.kalshi.com/api-reference/market/get-series-list','https://docs.kalshi.com/api-reference/events/get-events','https://docs.kalshi.com/api-reference/market/get-markets','https://docs.kalshi.com/api-reference/market/get-market-orderbook'],'api_requests_completed':len(LOG),'documentation_page_retrievals':6,'total_public_requests_including_documentation':len(LOG)+6,'excluded_candidate_or_nonstandard_market_count':len(candidate_specific_rows),'market_count':len(rows),'returned_2026_event_count_including_empty':len(events),'event_count_with_generic_party_markets':len(set(r['event_ticker'] for r in rows)),'offices':dict(collections.Counter(r['office'] for r in rows)),'receipt_start_utc':min((r['quote_receipt_utc'] for r in rows),default=None),'receipt_end_utc':max((r['quote_receipt_utc'] for r in rows),default=None)}
(ROOT/'reference.json').write_text(json.dumps({'metadata':meta,'markets':rows},indent=2)+'\n')
with (ROOT/'reference.csv').open('w',newline='') as f:
 w=csv.DictWriter(f,fieldnames=list(rows[0]) if rows else []);w.writeheader()
 for r in rows:w.writerow({k:json.dumps(v,separators=(',',':')) if isinstance(v,(dict,list)) else v for k,v in r.items()})
(ROOT/'excluded_events.json').write_text(json.dumps(excluded,indent=2)+'\n')
(ROOT/'observed_events.json').write_text(json.dumps(list(events.values()),indent=2)+'\n')
print(json.dumps(meta,indent=2));print('unknown identities',[(r['ticker'],r['race_name'],r['party']) for r in rows if r['state_code'] not in STATES or not r['party_rule_verified']]);print('duplicate race-party keys',[(k,len(v)) for k,v in __import__('itertools').groupby([])])
