"""Frozen Tartarus D falsifier; no collection, authority or default fitted model.

analyze_capture(manifest_path, sha256) independently reruns the fixed original
trace verifier before extracting known-sampler callbacks/full books. It does not
read active cohorts. evaluate_capture(capture) is the pure synthetic/data seam.
Baseline predictions must be supplied as an explicitly pinned declared artifact
by a future audited adapter; absent such an adapter they are unavailable. This
module does not turn synthetic source_innovation fixtures into a live model.

Protocol is immutable JSON beside research/tartarus-20261017. Every capacity
number is displayed-depth crossing counterfactual, not physical fills. Actual
execution-distribution outcomes are reported separately, never fabricated from
price labels. Old inspected datasets remain exploratory. Missing controls,
labels, source anchors and execution observations remain unavailable, not zero.
"""
import base64
from copy import deepcopy
from decimal import Decimal
import hashlib
import json
import math
from pathlib import Path
import types
from datetime import datetime

ROOT = Path(__file__).resolve().parents[2]
PROTOCOL_PATH = ROOT/'research/tartarus-20261017/liquidity-regime-protocol.json'
REPLAY_PATH = ROOT/'research/moloch-trade-trigger-20261016/witness/trace_replay.py'
REPLAY_LF_SHA = '3b7895720a06ee52104c9a23cbaf06fd0987e90405fac71673afaa0bdd602bc1'
PROTOCOL_LF_SHA = '0289f3ee5560a6793dd5bf8b42fde54a8fdb6dec403229615b6b655d5247b646'
UNIT = 1000000
MAX_ROWS = 200000
MAX_CAPTURE_BYTES = 64*1024*1024


def need(ok, code):
    if not ok:
        raise ValueError('REGIME_'+code)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def compact(value):
    return json.dumps(value,sort_keys=True,separators=(',',':'),allow_nan=False).encode()


def integer(value, code, low=0):
    need(type(value) is int and low<=value<=2**63-1,code)
    return value


def protocol():
    raw=PROTOCOL_PATH.read_bytes().replace(b'\r\n',b'\n').lstrip(b'\xef\xbb\xbf')
    need(digest(raw)==PROTOCOL_LF_SHA,'FROZEN_PROTOCOL_CHANGED')
    return json.loads(raw)


def ms(value):
    parsed=datetime.fromisoformat(value.replace('Z','+00:00'))
    need(parsed.tzinfo is not None,'TIMEZONE_REQUIRED')
    return int(parsed.timestamp()*1000)


def micros(value):
    need(not isinstance(value,bool),'DECIMAL')
    amount=Decimal(str(value))*UNIT
    need(amount.is_finite() and amount==amount.to_integral_value(),'EXACT_MICROS')
    return int(amount)


def levels(book, side):
    rows=book[side]
    need(isinstance(rows,list) and 0<len(rows)<=10000,'DEPTH')
    out=[]
    for level in rows:
        p=micros(level['price']);q=Decimal(str(level['quantity']))
        need(0<=p<=UNIT and q.is_finite() and q>0,'DEPTH_LEVEL')
        out.append((p,q))
    need(len(set(p for p,q in out))==len(out),'DUPLICATE_PRICE')
    return sorted(out,reverse=side=='bids')


def prefix(rows, quantity, haircut_bps=10000):
    remaining=Decimal(quantity);total=Decimal(0)
    for p,q in rows:
        take=min(remaining,q*Decimal(haircut_bps)/10000)
        total+=take*p;remaining-=take
        if remaining==0:
            return total
    return None


def crossing(entry, exit_book, side, quantity, haircut_bps=10000):
    """Exact decimal micro-unit totals; partial displayed level sizes preserved."""
    integer(quantity,'QUANTITY',1)
    if side=='YES':
        buy=levels(entry,'asks');sell=levels(exit_book,'bids')
    else:
        need(side=='NO','SIDE')
        buy=[(UNIT-p,q) for p,q in levels(entry,'bids')]
        sell=[(UNIT-p,q) for p,q in levels(exit_book,'asks')]
    cost=prefix(buy,quantity,haircut_bps);proceeds=prefix(sell,quantity,haircut_bps)
    if cost is None or proceeds is None:
        return None
    p=protocol();charge=quantity*(p['round_trip_fee_micros_assumption']+p['buffer_per_share_micros'])
    return {'entry_cost_micros':str(cost),'exit_proceeds_micros':str(proceeds),
            'net_micros':str(proceeds-cost-charge),'quantity':quantity,
            'displayed_counterfactual':True,'actual_fill':False}


def _book_event(record):
    b=record['book'];source=ms(b['sourceAt']);received=ms(b['receivedAt'])
    available=integer(record['available_at_ms'],'BOOK_AVAILABILITY')
    need(source<=available and received<=available,'BOOK_CLOCK')
    bids,asks=levels(b,'bids'),levels(b,'asks')
    need(bids[0][0]<=asks[0][0],'CROSSED_BOOK')
    return {'raw':b,'scope':(str(b['marketId']),str(b['exchangeId'])),
            'source':source,'received':received,'available':available,
            'hash':record['journal_hash'],'sequence':str(b['sequence']),
            'bid':bids[0],'ask':asks[0],'ordinal':record['ordinal']}


def barrier_applies(gap, scope):
    value=gap.get('scope')
    return value is None or (str(value[0])==scope[0] and (value[1] is None or str(value[1])==scope[1]))


def build_episodes(capture):
    p=protocol();decisions=capture['decisions'];books=capture['books'];barriers=capture.get('barriers',[])
    need(isinstance(decisions,list) and isinstance(books,list) and len(decisions)+len(books)<=MAX_ROWS,'ROW_BOUND')
    candidates={};conflicts=set()
    for d in sorted(decisions,key=lambda x:(x['decisionMs'],x['sampleId'])):
        t=d['trigger']['record'];key=(t['ticker'],t['tradeId'])
        economic={k:t.get(k) for k in ('ticker','tradeId','sourceTsMs','quantity','yesPrice','noPrice','direction','isBlockTrade')}
        for key_decimal in ('quantity','yesPrice','noPrice'):
            economic[key_decimal]=str(Decimal(str(economic[key_decimal])).normalize())
        if key in candidates:
            if candidates[key][1]!=economic:conflicts.add(key)
        else:candidates[key]=(d,economic)
    by_scope={};invalid_books=0
    for record in books:
        try:
            b=_book_event(record);by_scope.setdefault(b['scope'],[]).append(b)
        except (ValueError,KeyError):invalid_books+=1
    for values in by_scope.values():values.sort(key=lambda b:b['ordinal'])
    episodes=[];last={};counts={'event_conflicts':len(conflicts),'duplicate_events':len(decisions)-len(candidates),'cluster_skips':0,'invalid_books':invalid_books}
    for key,(d,economic) in sorted(candidates.items(),key=lambda x:(x[1][0]['decisionMs'],x[1][0]['sampleId'])):
        if key in conflicts:continue
        at=integer(d['decisionMs'],'DECISION')
        if key[0] in last and at-last[key[0]]<p['ticker_episode_spacing_ms']:
            counts['cluster_skips']+=1;continue
        last[key[0]]=at
        row={'event_key':list(key),'sample_id':d['sampleId'],'decision_ms':at,
             'scope':[d['marketId'],d['exchangeId']],'status':'censored','reason':None,
             'capacity':{},'regimes':{},'actual_execution':None}
        episodes.append(row)
        scope=(str(d['marketId']),str(d['exchangeId']));history=by_scope.get(scope,[])
        before=[b for b in history if b['available']<=at and b['hash']==d['sig']['journalHash']
                and ('sourceAt' not in d['sig'] or d['sig']['sourceAt']==b['raw']['sourceAt'])
                and ('receivedAt' not in d['sig'] or d['sig']['receivedAt']==b['raw']['receivedAt'])]
        if not before:row['reason']='missing_original_entry_book';continue
        entry=before[-1]
        if at-entry['source']>p['entry_maximum_age_ms'] or at-entry['received']>p['entry_maximum_age_ms']:
            row['reason']='stale_entry';continue
        target=at+p['horizon_ms']
        later=[b for b in history if b['ordinal']>entry['ordinal'] and b['source']>=target and b['received']>=target]
        if not later:row['reason']='missing_future_book';continue
        future=later[0]
        if capture.get('evidence_class')=='replayed_original_callbacks':
            labels=[v for v in capture['labels'] if v['sampleId']==d['sampleId'] and v['horizonMs']==p['horizon_ms']]
            if len(labels)!=1 or labels[0]['sigJournalHash']!=future['hash'] or labels[0].get('sourceAt')!=future['raw']['sourceAt'] or labels[0].get('receivedAt')!=future['raw']['receivedAt']:
                row['reason']='missing_or_different_original_horizon_label';continue
        if future['source']>target+p['label_slack_ms'] or future['received']>target+p['label_slack_ms']:
            row['reason']='first_future_book_outside_slack';continue
        if any(entry['ordinal']<g['ordinal']<=future['ordinal'] and barrier_applies(g,scope) for g in barriers):
            row['reason']='gap_or_rotation';continue
        previous=[b for b in history if b['ordinal']<entry['ordinal'] and b['available']<=at]
        anchor=previous[-1] if previous else None
        if anchor is None or not 0<=entry['source']-anchor['source']<=p['regime']['maximum_replenishment_anchor_age_ms'] or any(anchor['ordinal']<g['ordinal']<=entry['ordinal'] and barrier_applies(g,scope) for g in barriers):
            row['reason']='missing_causal_replenishment_anchor';continue
        row.update(status='labeled_display_counterfactual',entry_book_hash=entry['hash'],
                   exit_book_hash=future['hash'],label_available_at_ms=future['available'],
                   entry_source_at_ms=entry['source'],entry_received_at_ms=entry['received'])
        narrow=entry['ask'][0]-entry['bid'][0]<=p['regime']['maximum_spread_micros']
        young=at-entry['source']<=p['regime']['maximum_revision_age_ms']
        for side,quote in [('YES','ask'),('NO','bid')]:
            row['regimes'][side]=int(narrow and young and entry[quote][0]==anchor[quote][0] and entry[quote][1]>anchor[quote][1])
            row['capacity'][side]={str(q):{'normal':crossing(entry['raw'],future['raw'],side,q),
                                           'depth_haircut':crossing(entry['raw'],future['raw'],side,q,p['controls']['depth_haircut_bps'])} for q in p['quantity_grid']}
        row['controls']={'local_bbo':{side:-(entry['ask'][0]-entry['bid'][0])-p['buffer_per_share_micros']-p['round_trip_fee_micros_assumption'] for side in ('YES','NO')},
                         'delayed_external':None,'wrong_race':None,'adverse_queue':None,'doubled_latency':None,'leave_one_event_out':None}
    return episodes,counts


def evaluate_capture(capture):
    """Counts/capacity remain operable when genuine baseline/OOS data is absent."""
    episodes,counts=build_episodes(capture)
    labeled=[e for e in episodes if e['status']=='labeled_display_counterfactual']
    learning=protocol()['learning']
    boundary=episodes[len(episodes)*learning['training_fraction_numerator']//learning['training_fraction_denominator']]['decision_ms'] if len(episodes)>1 else None
    training=[e for e in labeled if boundary is not None and e['decision_ms']<boundary and e['label_available_at_ms']<boundary]
    heldout=[e for e in labeled if boundary is not None and e['decision_ms']>=boundary]
    insufficiency=['GENUINE_SOURCE_MODEL_BASELINE_UNAVAILABLE','COMMON_ALL_CONTROL_EPISODES_UNAVAILABLE']
    if len(training)<learning['minimum_training_episodes']:insufficiency.append('INSUFFICIENT_TRAINING_EPISODES')
    if len(heldout)<learning['minimum_oos_episodes']:insufficiency.append('INSUFFICIENT_OOS_EPISODES')
    execution=capture.get('execution_outcomes')
    if execution is not None:
        need(isinstance(execution,list) and len(execution)<=4096,'EXECUTION_OUTCOME_BOUND')
        for outcome in execution:
            need(isinstance(outcome,dict) and outcome.get('class') in ('confirmed_zero','confirmed_partial','confirmed_full','unresolved','proved_no_network','observer_unavailable'),'EXECUTION_CLASS')
            requested=integer(outcome.get('requested_quantity'),'REQUESTED_QUANTITY',1)
            filled=integer(outcome.get('observed_filled_quantity'),'FILLED_QUANTITY')
            need(filled<=requested,'FILLED_EXCEEDS_REQUESTED')
            kind=outcome['class']
            need(kind!='confirmed_zero' or filled==0,'EXECUTION_CLASS_QUANTITY')
            need(kind!='confirmed_full' or filled==requested,'EXECUTION_CLASS_QUANTITY')
            need(kind!='confirmed_partial' or 0<filled<requested,'EXECUTION_CLASS_QUANTITY')
            need(isinstance(outcome.get('source_sha256'),str) and len(outcome['source_sha256'])==64 and all(c in '0123456789abcdef' for c in outcome['source_sha256']),'EXECUTION_SOURCE_SHA')
    curve=[]
    for side in ('YES','NO'):
        for q in protocol()['quantity_grid']:
            for control in ('normal','depth_haircut'):
                outcomes=[Decimal(e['capacity'][side][str(q)][control]['net_micros']) for e in labeled if e['capacity'][side][str(q)][control] is not None]
                curve.append({'side':side,'quantity':q,'control':control,'displayed_capacity_episodes':len(outcomes),
                              'counterfactual_net_sum_micros':str(sum(outcomes)) if outcomes else None,
                              'count':len(outcomes),'model_selected':False,'actual_execution_count':None})
    return {'schema_version':1,'status':'insufficient_evidence','episode_dataset_manifest':{
                'protocol_sha256':digest(PROTOCOL_PATH.read_bytes().replace(b'\r\n',b'\n').lstrip(b'\xef\xbb\xbf')),
                'input_manifest_sha256':capture.get('input_manifest_sha256'),
                'input_replay':capture.get('input_replay'),'evidence_class':capture.get('evidence_class'),
                'cohort_selection_frozen_before_outcomes':None,'prospective_validation_qualified':False,
                'episode_count':len(episodes),'counts':counts,'episodes_sha256':digest(compact(episodes)),
                'selection_before_completeness':True},
            'episodes':episodes,'capacity_curve':curve,
            'chronology':{'boundary_decision_ms':boundary,'boundary_before_completeness':True,
                          'training_sample_ids':[e['sample_id'] for e in training],
                          'heldout_sample_ids':[e['sample_id'] for e in heldout],
                          'training_label_availability_strictly_before_boundary':True,
                          'common_all_control_episode_count':0,
                          'leave_one_event_out_groups':[e['event_key'] for e in episodes],
                          'leave_one_event_out_evaluated':False},
            'insufficiency_reasons':insufficiency,
            'source_model_baseline':{'status':'unavailable','reason':'GENUINE_TRAINED_SOURCE_MODEL_ARTIFACT_AND_PREDICTION_ADAPTER_NOT_SUPPLIED'},
            'trained_artifact':None,'quantity_model':None,
            'actual_execution_observations':{'rows':deepcopy(execution),'original_receipt_membership_independently_verified':False,
                                           'informative_cancellations_retained':True,'independent_censoring_assumed':False},
            'rejected_variants':[{'id':'regime_residual_correction','status':'not_fitted','reason':'BASELINE_OR_COMMON_HELDOUT_CONTROL_EVIDENCE_UNAVAILABLE'}],
            'missing_controls':['delayed_external','wrong_race','adverse_queue','doubled_latency','leave_one_event_out'],
            'unmodeled_state_statement':'Displayed outcomes do not establish own fills. Provider completeness, topic/epoch, economic mapping and actual queue conditions remain unresolved.',
            'execution_authorized':False,'calibrated_probability_claimed':False}


def analyze_capture(manifest_path, expected_sha256):
    """Replay only known reviewed repository code; no manifest-chosen code paths."""
    raw=REPLAY_PATH.read_bytes();need(digest(raw.replace(b'\r\n',b'\n'))==REPLAY_LF_SHA,'FIXED_REPLAY_CHANGED')
    replay=types.ModuleType('_fixed_regime_replay');replay.__file__=str(REPLAY_PATH)
    exec(compile(raw,str(REPLAY_PATH),'exec'),replay.__dict__)
    proof=replay.verify(manifest_path,expected_sha256)
    manifest_pin=replay.Pinned({'path':str(Path(manifest_path).absolute()),'sha256':expected_sha256,'bytes':Path(manifest_path).stat().st_size})
    manifest=replay.loads(manifest_pin.small(1024**2));module=replay.known_sampler(manifest['producer']['sha256'])
    capture={'decisions':[],'books':[],'labels':[],'barriers':[],'input_manifest_sha256':expected_sha256,'input_replay':proof,'evidence_class':'replayed_original_callbacks'}
    capture_bytes=0
    pins=[replay.Pinned(manifest[k]) for k in ('trace','samples','seal','producer','mapping')]
    ordinal=0;study=None;body=None;source_format=None;sources=replay.Sources(manifest['sigSources'])
    def emit(value):
        nonlocal capture_bytes
        if value.get('kind')=='decision_features' and 'trigger' in value:
            capture['decisions'].append(deepcopy(value));capture_bytes+=len(compact(value))
        elif value.get('kind')=='horizon_label':
            capture['labels'].append(deepcopy(value));capture_bytes+=len(compact(value))
        need(capture_bytes<=MAX_CAPTURE_BYTES,'CAPTURE_MEMORY_BOUND')
    for record in replay.lines(replay.Pinned(manifest['trace'])):
        event=record['event'];kind=event['kind'];ordinal+=1
        if kind=='run_start':
            source_format=event['effectiveSamplerConfig']['format']
            sources.start(event['initialBookmark']);study=module.Study(emit,sampling_mode=event['effectiveSamplerConfig']['samplingMode'])
            original_accept=study.accept
            def accept(row,at):
                nonlocal capture_bytes
                prior=study.counts['sigBooks']
                original_accept(row,at)
                e=row['event']
                if e.get('type')=='book' and study.counts['sigBooks']>prior:
                    capture['books'].append({'book':deepcopy(e['book']),'journal_hash':row['hash'],'available_at_ms':at,'ordinal':ordinal});capture_bytes+=len(compact(e['book']))
                elif e.get('type')=='gap':capture['barriers'].append({'ordinal':ordinal,'scope':[str(e['marketId']),str(e['exchangeId']) if e.get('exchangeId') is not None else None] if e.get('marketId') is not None else None})
                need(len(capture['books'])+len(capture['decisions'])<=MAX_ROWS,'ROW_BOUND')
                need(capture_bytes<=MAX_CAPTURE_BYTES,'CAPTURE_MEMORY_BOUND')
            study.accept=accept
        elif kind=='poll_body':body=base64.b64decode(event['bodyBytesBase64'],validate=True)
        elif kind=='apply_begin':
            if event['inputKind']=='cache_body':study.external_update(replay.loads(body),event['samplerNowMs']);body=None
            else:
                row=sources.consume(event['input']);(study.breadth if source_format=='breadth' else study.accept)(row,event['samplerNowMs'])
        elif kind=='sig_rotation':
            sources.rotate(event);capture['barriers'].append({'ordinal':ordinal,'scope':None})
    for original in [manifest_pin,*pins,*sources.pins]:original.recheck()
    return evaluate_capture(capture)
