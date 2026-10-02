"""Local-only read-only lead screen; no network or trading."""
import json, csv, re, hashlib
from pathlib import Path
from decimal import Decimal as D
from datetime import datetime, timezone
from collections import Counter, defaultdict
BASE=Path('/workspace/shared/predictions_cup'); OUT=BASE/'crossvenue_screen_2026-10-02'
NOW=datetime.now(timezone.utc)
def load(p): return json.loads(Path(p).read_text(),parse_float=D)
def s(v): return None if v is None else format(D(v),'f')
def ago(t): return round((NOW-datetime.fromisoformat(t.replace('Z','+00:00'))).total_seconds(),3)
def dump(name,value): (OUT/name).write_text(json.dumps(value,indent=2,default=lambda o:s(o) if isinstance(o,D) else str(o))+'\n')
identity=load(BASE/'cup_ui_identity_map_2026-10-02.json'); bulk=load(OUT/'cup_bulk_leads.json'); ref=load(BASE/'public_election_reference_2026-10-02/reference.json'); books=load(BASE/'public_election_reference_2026-10-02/orderbook_snapshots.json')['books']
leads={str(e['marketId']):e for e in bulk['exchanges']}; bookidx={b['ticker']:b for b in books}
states={m['state_name']:m['state_code'] for m in ref['markets']}
idx=defaultdict(list)
for m in ref['markets']: idx[(m['office'],m['state_code'],m['district'],m['party'])].append(m)
complement_evidence={'url':'https://sig.thesuper.market/docs/markets-and-trading','saved_evidence_file':str(BASE/'first_place_strategy_requirements_2026-10-01.md'),'line':36,'interpretation':'Single-contract YES/NO share one complementary book. Cup NO ask = 1 - same-contract YES bid. No quantity derived because bulk source has none. Distinct party contracts are never treated as complements.'}
rows=[]; unmatched=[]; ambiguities=[]; joined=[]
for c in identity['markets']:
    mid=str(c['market_id']); lead=leads.get(mid); office=state=district=None
    if (m:=re.fullmatch(r'([A-Z]{2})-(\d\d) House race',c['race'])): office='House';state=m[1];district=str(int(m[2]))
    elif (m:=re.fullmatch(r'(.+) (Senate|Governor)',c['race'])) and m[1] in states: office=m[2];state=states[m[1]]
    key=(office,state,district,c['party']); candidates=idx.get(key,[])
    base={'cup_market_id':mid,'cup_title':c['title'],'cup_party':c['party'],'cup_race':c['race'],'cup_url':c['url'],'cup_office':office,'cup_state_code':state,'cup_district':district,'cup_election_year':2026,'cup_election_year_source_url':'https://predictionscup.com/rules/','cup_election_year_evidence':'Official tournament-wide 2026 Midterm Elections scope, supplied and verified by parent; scoped market IDs are from same Cup inventory','cup_special_election':None,'cup_stage':c.get('stage'),'cup_identity_observed_at':c['observed_at'],'cup_identity_live_reverified_today':c['live_reverified_today'],'cup_identity_provenance':c['provenance'],'cup_received_at':lead.get('receivedAt') if lead else None,'cup_source_quote_age_seconds':None,'cup_quote_receipt_age_seconds':ago(lead['receivedAt']) if lead else None,'cup_best_yes_bid':s(lead.get('bestBid')) if lead else None,'cup_best_yes_ask':s(lead.get('bestAsk')) if lead else None,'cup_quantity':None,'cup_depth_status':'missing_from_bulk_schema','strict_join_status':'blocked_pending_cup_regular_special_stage_and_resolution_rules','conditional_reference_matches':len(candidates)}
    joined.append(base)
    if not candidates:
        reason='chamber_control_not_in_reference_scope' if c['race'].startswith('U.S.') else 'generic_independent_not_present_no_candidate_substitution' if c['party']=='Independent' else 'no_exact_office_state_district_party_reference'
        unmatched.append({**base,'reason':reason}); continue
    if len(candidates)>1:
        ambiguities.append({**base,'reference_tickers':[m['ticker'] for m in candidates],'reason':'multiple_office_state_district_party_candidates'});continue
    r=candidates[0]; flags=['cup_year_from_tournament_scope_not_individual_rule','cup_regular_special_unverified','cup_stage_unverified','cup_contract_full_resolution_rules_not_reverified','crossvenue_settlement_equivalence_not_established','cup_certified_winner_vs_kalshi_inauguration_or_seating_basis_risk','party_fusion_candidate_affiliation_treatment_requires_check','accelerated_media_determination_timing_difference','cancellation_refund_sponsor_override_requires_check','cup_stale_mixed_receipt_no_depth','both_source_quote_ages_unknown','fees_slippage_fillability_not_accounted']
    if not c['live_reverified_today']: flags.append('cup_identity_relies_on_oct1_catalog')
    if r['special_election']: flags.append('reference_explicit_special_election_cup_type_unverified')
    if state=='MN' and c['party']=='Democratic': flags.append('democratic_vs_DFL_wording_check')
    if state=='AK' and office=='Governor': flags.append('Alaska_joint_ticket_or_nonpartisan_affiliation_check')
    if c['party']=='Independent':flags.append('generic_independent_party_definition_requires_explicit_equivalence')
    if not lead or lead.get('status')!='observed': continue
    if lead.get('crossed'):flags.append('cup_crossed_book_reported')
    for side in ['YES','NO']:
        ask=lead.get('bestAsk') if side=='YES' else (D(1)-D(lead['bestBid']) if lead.get('bestBid') is not None else None)
        rb=r['yes_bid_dollars' if side=='YES' else 'no_bid_dollars']; ra=r['yes_ask_dollars' if side=='YES' else 'no_ask_dollars']; size=r['yes_bid_size_fp' if side=='YES' else 'no_bid_size_fp']; rtime=r['quote_receipt_utc']; rsource=r['market_details_api_url']; rkind='event_market_top_direct'; b=bookidx.get(r['ticker']); bfield='yes_bid_dollars' if side=='YES' else 'no_bid_dollars'; sizefield='yes_bid_size_fp' if side=='YES' else 'no_bid_size_fp'
        # Do not splice later full-book size into an earlier event price. If used, replace all reference-side fields with that book snapshot.
        if b:
            rb=b[bfield];size=b[sizefield];ra=b['implied_yes_ask_dollars' if side=='YES' else 'implied_no_ask_dollars'];rtime=b['receipt_utc'];rsource=b['source_url'];rkind='full_orderbook_direct_bid_and_size_with_complement_ask'
        eligible=ask is not None and rb is not None and D(0)<=D(ask)<=D(1) and D(0)<=D(rb)<=D(1) and (size is None or D(size)>0) and not lead.get('crossed')
        extra=[]
        if side=='NO': extra+=['cup_NO_ask_derived_from_same_contract_YES_bid']
        if size is None: extra+=['public_reference_bid_size_unknown']
        if size is not None and D(size)==0:extra+=['public_reference_bid_size_zero_quote_not_usable']
        row={**base,'side':side,'cup_stale_ask':s(ask),'cup_ask_is_derived':side=='NO','cup_ask_derivation': '1 - same-contract bestBid' if side=='NO' else None,'cup_ask_derivation_source':complement_evidence if side=='NO' else None,'reference_ticker':r['ticker'],'reference_event_ticker':r['event_ticker'],'reference_race_key':r['race_key'],'reference_election_year':r['election_year'],'reference_special_election':r['special_election'],'reference_bid':s(rb),'reference_bid_size':s(size),'reference_ask':s(ra),'reference_snapshot_kind':rkind,'reference_quote_receipt_utc':rtime,'reference_receipt_age_seconds':ago(rtime),'reference_source_quote_age_seconds':None,'reference_url':rsource,'reference_market_url':r['market_details_api_url'],'reference_contract_terms_url':r['contract_terms_url'],'reference_rules_primary':r['rules_primary'],'reference_rules_secondary':r['rules_secondary'],'reference_settlement_sources':r['settlement_sources'],'quote_fields_available_for_conditional_lead':eligible,'conditional_bid_minus_cup_ask_gap':s(D(rb)-D(ask)) if eligible else None,'conditional_bid_minus_cup_ask_gap_cents':s((D(rb)-D(ask))*100) if eligible else None,'reference_ask_minus_cup_ask_upper_band_difference':s(D(ra)-D(ask)) if ask is not None and ra is not None else None,'reference_ask_band_difference_not_expected_return':True,'maximum_possible_terminal_gain_per_share_before_costs':s(D(1)-D(ask)) if ask is not None else None,'maximum_possible_terminal_loss_per_share_before_costs':s(ask),'gap_interpretation':'Conditional asynchronous quote difference only; neither an executable trade nor a valuation/probability/arb signal','semantic_and_data_flags':flags+extra}
        rows.append(row)
ranked=sorted([r for r in rows if r['conditional_bid_minus_cup_ask_gap'] is not None],key=lambda r:(D(r['conditional_bid_minus_cup_ask_gap']),r['cup_market_id'],r['side']),reverse=True)
positive=[r for r in ranked if D(r['conditional_bid_minus_cup_ask_gap'])>0]
# Separate-party diagnostic only. Never assert exact complements, guaranteed payoff, or executable size.
pairgroups=defaultdict(dict)
for c in identity['markets']:
    if c['party'] in ['Democratic','Republican']:pairgroups[c['race']][c['party']]=c
pairs=[]
for race,cs in pairgroups.items():
    if len(cs)!=2:continue
    ds=cs['Democratic'];rs=cs['Republican'];d=leads[str(ds['market_id'])];r=leads[str(rs['market_id'])]
    asks=[d.get('bestAsk'),r.get('bestAsk')];bids=[d.get('bestBid'),r.get('bestBid')]
    pairs.append({'race':race,'democratic_market_id':str(ds['market_id']),'republican_market_id':str(rs['market_id']),'democratic_url':ds['url'],'republican_url':rs['url'],'democratic_yes_bid':s(bids[0]),'democratic_yes_ask':s(asks[0]),'republican_yes_bid':s(bids[1]),'republican_yes_ask':s(asks[1]),'democratic_received_at':d['receivedAt'],'republican_received_at':r['receivedAt'],'sum_yes_asks':s(sum(asks)) if all(x is not None for x in asks) else None,'one_minus_sum_yes_asks':s(D(1)-sum(asks)) if all(x is not None for x in asks) else None,'sum_yes_bids_minus_one':s(sum(bids)-D(1)) if all(x is not None for x in bids) else None,'same_receipt_batch':d['receivedAt']==r['receivedAt'],'known_quantity':None,'interpretation':'Separate-party pair price diagnostic only. Third-party, fusion, stage/year, cancellation and settlement-rule differences prevent guaranteed $1 combined payoff or exact complement assumption.'})
pairs.sort(key=lambda p:D(p['one_minus_sum_yes_asks']) if p['one_minus_sum_yes_asks'] else D('-9'),reverse=True)
summary={'generated_at_utc':NOW.isoformat(),'local_analysis_network_requests':0,'additional_Cup_or_public_reference_requests':0,'read_only_GitHub_source_fetches':2,'orders_by_screen':0,'bulk_rows':len(leads),'identity_rows':len(identity['markets']),'exact_id_intersection':len(set(leads)&{str(c['market_id']) for c in identity['markets']}),'strict_full_identity_joins':0,'strict_executable_crossvenue_opportunities':0,'conditional_unique_office_state_district_party_joins':sum(j['conditional_reference_matches']==1 for j in joined),'unmatched_cup_markets':len(unmatched),'ambiguous_multiple_reference_candidates':len(ambiguities),'conditional_side_rows':len(rows),'conditional_quote_gap_rows':len(ranked),'conditional_positive_gap_side_rows':len(positive),'conditional_positive_gap_distinct_cup_markets':len({r['cup_market_id'] for r in positive}),'cup_year_from_official_tournament_context':237,'cup_regular_special_verified':0,'identity_live_reverified_today':identity['live_reverified_count'],'unmatched_reason_counts':dict(Counter(m['reason'] for m in unmatched)),'public_contracts':len(ref['markets']),'public_source_quote_age_known':0,'cup_source_quote_age_known':0,'cup_depth_rows':0,'source_commit':'3175b7c12c12626f15c925eaf1d5a6ba27a215aa','bulk_file_url':'https://github.com/GodlyDonuts/predictions-cup-2026/blob/3175b7c12c12626f15c925eaf1d5a6ba27a215aa/data/evidence/breadth-20261002-0525/leads.json','bulk_sha256':hashlib.sha256((OUT/'cup_bulk_leads.json').read_bytes()).hexdigest(),'bulk_sha256_matches_manifest':hashlib.sha256((OUT/'cup_bulk_leads.json').read_bytes()).hexdigest()=='4054acf1eacca0db6f5b641f7b205a0ed6366da413fb1c7fce13d1a7e732d9ec','complement_evidence':complement_evidence,'strict_join_note':'Cup year 2026 is supported by official tournament scope; individual titles still omit stage, regular/special type, and full resolution rules. Race-party matches are conditional UI-verification leads, not asserted settlement-equivalent contracts. Florida Senate is explicitly special in the public reference and remains a flagged type-check lead.'}
dump('all_joined_side_observations.json',rows);dump('summary.json',summary);dump('join_records.json',joined);dump('conditional_crossvenue_leads.json',ranked);dump('unmatched_and_ambiguous.json',{'unmatched':unmatched,'ambiguous':ambiguities,'strict_join_blocked':joined});dump('top10_candidates.json',ranked[:10]);dump('cup_separate_party_pair_diagnostics.json',pairs)
fields=['cup_market_id','cup_title','side','cup_url','cup_stale_ask','cup_ask_is_derived','cup_quantity','reference_ticker','reference_bid','reference_bid_size','conditional_bid_minus_cup_ask_gap','conditional_bid_minus_cup_ask_gap_cents','reference_ask_minus_cup_ask_upper_band_difference','maximum_possible_terminal_gain_per_share_before_costs','cup_received_at','cup_quote_receipt_age_seconds','reference_quote_receipt_utc','reference_receipt_age_seconds','reference_url','reference_contract_terms_url','strict_join_status','semantic_and_data_flags']
with (OUT/'conditional_crossvenue_leads.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=fields);w.writeheader()
    for r in ranked:w.writerow({k:'; '.join(r[k]) if isinstance(r.get(k),list) else r.get(k) for k in fields})
print(json.dumps(summary,indent=2)); print('\nTOP 20')
for r in ranked[:20]:print(r['cup_market_id'],r['cup_race'],r['cup_party'],r['side'],'Cup',r['cup_stale_ask'],'Ref',r['reference_bid'],'size',r['reference_bid_size'],'GAP cents',r['conditional_bid_minus_cup_ask_gap_cents'],r['reference_ticker'],'fresh identity',r['cup_identity_live_reverified_today'])
print('\nUNMATCHED',[(r['cup_market_id'],r['cup_title'],r['reason']) for r in unmatched]);print('\nPAIRS',json.dumps(pairs[:6],indent=2))
