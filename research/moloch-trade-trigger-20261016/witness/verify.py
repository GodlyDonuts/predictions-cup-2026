"""Independent original-body membership; deliberately not a complete observer trace."""
import argparse
from collections import defaultdict
from decimal import Decimal
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import sys

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('trade_analysis', HERE.parent/'analyze.py')
a = importlib.util.module_from_spec(spec); spec.loader.exec_module(a)
sys.path.insert(0, str(HERE.parents[2]/'moloch/external-data'))
import crossvenue_observer as o
MAX_BYTES, MAX_LINE, MAX_ROWS = 512*1024*1024, 8*1024*1024, 500000


def need(ok, code):
    if not ok:
        raise ValueError('ORIGINAL_WITNESS_' + code)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def signature(path):
    s = path.lstat()
    need(stat.S_ISREG(s.st_mode), 'REGULAR_FILE_REQUIRED')
    return s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns


def pinned(base, descriptor):
    need(set(descriptor) == {'path', 'bytes', 'sha256'}, 'FILE_DESCRIPTOR')
    name = Path(descriptor['path'])
    need(not name.is_absolute() and '..' not in name.parts, 'RELATIVE_SOURCE_PATH')
    path = base/name
    for parent in (path, *path.parents):
        need(not parent.is_symlink(), 'SYMLINK')
    before = signature(path)
    need(type(descriptor['bytes']) is int and 0 <= descriptor['bytes'] <= MAX_BYTES and before[2] == descriptor['bytes'], 'FILE_SIZE')
    raw = path.read_bytes()
    need(sha(raw) == descriptor['sha256'] and signature(path) == before, 'FILE_HASH')
    return path, before, raw


def lines(raw, compressed, budget=None):
    stream = gzip.GzipFile(fileobj=io.BytesIO(raw)) if compressed else io.BytesIO(raw)
    used = 0
    for count, line in enumerate(iter(lambda: stream.readline(MAX_LINE+1), b''), 1):
        used += len(line)
        if budget is not None:
            budget['rows'] += 1; budget['bytes'] += len(line)
            need(budget['rows'] <= MAX_ROWS and budget['bytes'] <= MAX_BYTES, 'TOTAL_DECODE_BOUNDS')
        need(len(line) <= MAX_LINE and line.endswith(b'\n') and count <= MAX_ROWS and used <= MAX_BYTES, 'DECODE_BOUNDS')
        yield line.rstrip(b'\r\n'), a.decode(line)


def sig_segment(raw, compressed, segment, budget=None):
    events, revision, versions, states = [], {}, {}, {}
    previous, sequence = '', 0
    for line, row in lines(raw, compressed, budget):
        need(set(row) == {'event', 'hash', 'previous', 'sequence'} and row['sequence'] == sequence+1 and row['previous'] == previous, 'SIG_CHAIN')
        token = b',"hash":"'+row['hash'].encode()+b'"'
        need(line.count(token) == 1 and sha(line.replace(token,b'',1)) == row['hash'], 'SIG_ROW_HASH')
        previous, sequence = row['hash'], row['sequence']
        e = row['event']; need(e['source'] == 'official_sig_realtime', 'SIG_SOURCE')
        received = o.millis(e['receivedAt'])
        def gap(market=None, exchange=None):
            events.append({'kind':'gap','market':market,'exchange':exchange,'segment':segment,'position':len(events)})
        if e['event'] == 'stream_reset':
            need(e['payload']['localBoundary'] is True, 'RESET'); gap(); revision.clear(); versions.clear(); continue
        if e['event'] != 'markets_batch':
            if e['event'] in ('book_dirty','market_settled'):
                gap(str(e['payload']['marketId']),str(e['payload']['exchangeId']) if 'exchangeId' in e['payload'] else None)
            continue
        for m in e['payload']['markets']:
            market, b = str(m['marketId']), m['batch']; d = b['delivery']
            need(d['model'] == 'best-effort-authoritative-resync' and type(d['revision']) is int and type(d['previousRevision']) is int and d['revision'] > d['previousRevision'] >= 0, 'DELIVERY')
            old = revision.get(market)
            if old is not None and d['revision'] <= old:
                continue  # Exact original observer breadth semantics, not native books.
            if old is not None and d['previousRevision'] != old or b.get('resyncRequired') or b.get('marketSettled'):
                gap(market)
                versions = {k:v for k,v in versions.items() if k[0] != market}
            revision[market] = d['revision']
            seen = set()
            for book in b.get('books', []):
                exchange = str(book['exchangeId']); key = market, exchange
                need(exchange not in seen, 'DUPLICATE_EXCHANGE'); seen.add(exchange)
                version = book['asOf']['sequence']; source = o.millis(book['asOf']['at'])
                need(type(version) is int and version >= 0 and source <= received, 'SIG_VERSION_CLOCK')
                def levels(values):
                    result = []
                    for level in values:
                        p = Decimal(str(level['price'])); tick = Decimal('.005')
                        canonical = (p/tick).quantize(Decimal(1))*tick
                        need(abs(p-canonical) <= Decimal('0.000000000001'), 'SIG_TICK')
                        result.append({'price':format(canonical,'.6f'),'quantity':level['quantity']})
                    return result
                bids, asks = levels(book['bids']),levels(book['asks'])
                identity = key,version,book['asOf']['at']
                state = a.compact({'bids':bids,'asks':asks})
                need(identity not in states or states[identity]==state, 'SIG_VERSION_CONFLICT')
                states[identity] = state
                if version <= versions.get(key, -1):
                    continue
                versions[key] = version
                if not bids or not asks:
                    gap(market,exchange); continue
                features = o.features(o.levels(bids,False),o.levels(asks,True))
                events.append({'kind':'book','segment':segment,'position':len(events),'market':market,'exchange':exchange,
                               'hash':row['hash'],'journalSequence':sequence,'version':str(version),'sourceAt':book['asOf']['at'],
                               'receivedAt':e['receivedAt'],'sourceMs':source,'receivedMs':received,'features':features})
    return events


def collector(files, budget=None):
    by_hash, identities = {}, {}
    for raw, compressed in files:
        for _, row in lines(raw,compressed,budget):
            if row.get('kind') != 'websocket' or row.get('type') != 'trade':
                continue
            need(row['source'] == 'kalshi_official_ws_v2' and sha(row['bodyText'].encode()) == row['bodySha256'] and a.decode(row['bodyText']) == row['body'], 'TRADE_BODY')
            f, m = row['body'], row['body']['msg']
            need(f['type'] == 'trade' and row['sid'] == f['sid'] and row['sequence'] == f.get('seq'), 'TRADE_FRAME')
            directions, ambiguous = [], False
            for k, mapping in [('taker_outcome_side',{'yes':1,'no':-1}),('taker_book_side',{'bid':1,'ask':-1}),('taker_side',{'yes':1,'no':-1})]:
                if m.get(k) is not None:
                    if m[k] not in mapping: ambiguous = True
                    else: directions.append(mapping[m[k]])
            direction = directions[0] if not ambiguous and directions and len(set(directions)) == 1 else None
            fact = {'ticker':m['market_ticker'],'tradeId':m['trade_id'],'sourceTsMs':m.get('ts_ms'),'receivedMs':o.millis(row['receivedAt']),
                    'sid':f['sid'],'sequence':f.get('seq'),'quantity':m['count_fp'],'yesPrice':m['yes_price_dollars'],'noPrice':m['no_price_dollars'],
                    'direction':direction,'isBlockTrade':m.get('is_block_trade')}
            identity = fact['ticker'],fact['tradeId']
            economic = {k:v for k,v in fact.items() if k not in ('sid','sequence','receivedMs')}
            need(identity not in identities or identities[identity] == economic, 'TRADE_IDENTITY_CONFLICT')
            identities[identity] = economic
            need(row['bodySha256'] not in by_hash or by_hash[row['bodySha256']] == fact, 'TRADE_RECEIPT_CONFLICT')
            by_hash[row['bodySha256']] = fact
    return by_hash


def membership(sample, events, trades):
    trigger, ref = sample['trigger']['record'],sample['sig']
    original = trades.get(trigger['bodySha256']); need(original is not None, 'MISSING_TRADE_BODY')
    need(all(trigger[k] == v for k,v in original.items()), 'TRADE_ORIGINAL_MISMATCH')
    candidates = [e for e in events if e['kind']=='book' and e['hash']==ref['journalHash'] and e['market']==sample['marketId'] and e['exchange']==sample['exchangeId']]
    need(len(candidates)==1, 'MISSING_OR_AMBIGUOUS_SIG_BOOK')
    book = candidates[0]
    need(book['journalSequence']==ref['journalSequence'] and book['sourceAt']==ref['sourceAt'] and book['receivedAt']==ref['receivedAt'] and book['features']==ref['features'], 'SIG_ORIGINAL_MISMATCH')
    return book


def first_recorded(sample, label, initial, events):
    original = [e for e in events if e['kind']=='book' and e['hash']==label['sigJournalHash'] and (e['market'],e['exchange'])==(sample['marketId'],sample['exchangeId'])]
    need(len(original)==1, 'MISSING_OR_AMBIGUOUS_LABEL_BODY')
    need(original[0]['sourceAt']==label['sourceAt'] and original[0]['receivedAt']==label['receivedAt'] and original[0]['features']==label['features'], 'LABEL_ORIGINAL_MISMATCH')
    target = sample['decisionMs']+label['horizonMs']
    for e in events:
        if e['segment'] != initial['segment'] or e['position'] <= initial['position']:
            continue
        if e['kind']=='gap' and e['market'] in (None,sample['marketId']) and e['exchange'] in (None,sample['exchangeId']):
            return 'unavailable_recorded_boundary'
        if e['kind']!='book' or (e['market'],e['exchange']) != (sample['marketId'],sample['exchangeId']) or e['sourceMs']<target or e['receivedMs']<target:
            continue
        need(e['sourceMs']-target<=3000 and e['receivedMs']-target<=3000, 'FIRST_RECORDED_BOOK_OUTSIDE_SLACK')
        need(e['hash']==label['sigJournalHash'] and e['sourceAt']==label['sourceAt'] and e['receivedAt']==label['receivedAt'] and e['features']==label['features'], 'FIRST_RECORDED_BOOK_MISMATCH')
        return 'verified_first_in_recorded_segment'
    return 'unavailable_no_candidate_in_same_recorded_segment'


def verify(manifest, expected):
    manifest = Path(manifest); base = manifest.parent
    before = signature(manifest); raw = manifest.read_bytes(); need(sha(raw)==expected,'MANIFEST_HASH')
    m = a.decode(raw)
    need(set(m)=={'schemaVersion','sealed','cohortId','asOfUtc','sampleFiles','sigSegments','collectorFiles','observerInputTrace'} and m['schemaVersion']==1 and m['sealed'] is True and isinstance(m['cohortId'],str) and m['cohortId'], 'MANIFEST_SCHEMA')
    need(m['observerInputTrace'] is None, 'TRACE_SCHEMA_NOT_IMPLEMENTED')
    for key in ('sampleFiles','sigSegments','collectorFiles'):
        need(isinstance(m[key],list) and 0<len(m[key])<=32,'SOURCE_LIST_REQUIRED')
    descriptors = m['sampleFiles']+[s['file'] for s in m['sigSegments']]+m['collectorFiles']
    need(all(set(s)=={'segmentId','file'} for s in m['sigSegments']) and len({s['segmentId'] for s in m['sigSegments']})==len(m['sigSegments']), 'SEGMENT_ID')
    pins = [pinned(base,d) for d in descriptors]
    need(len({p[0] for p in pins})==len(pins) and sum(len(p[2]) for p in pins)<=MAX_BYTES,'SOURCE_BOUNDS')
    budget = {'rows':0,'bytes':0}
    n, k = len(m['sampleFiles']),len(m['sigSegments'])
    events = [e for i,(p,_,b) in enumerate(pins[n:n+k]) for e in sig_segment(b,p.suffix=='.gz',m['sigSegments'][i]['segmentId'],budget)]
    trades = collector([(b,p.suffix=='.gz') for p,_,b in pins[n+k:]],budget)
    samples, labels = {}, []
    cutoff = a.clock(m['asOfUtc'])
    for p,_,b in pins[:n]:
        for _,row in lines(b,p.suffix=='.gz',budget):
            if row.get('kind')=='decision_features':
                a.decision(row,cutoff); sid=row['sampleId']; need(sid not in samples or samples[sid]==row,'SAMPLE_CONFLICT'); samples[sid]=row
            elif row.get('kind')=='horizon_label': labels.append(row)
    originals = {sid:membership(s,events,trades) for sid,s in samples.items()}
    results, seen_labels = [], set()
    for label in labels:
        sid,h = label['sampleId'],label['horizonMs']; need(sid in samples,'ORPHAN_LABEL'); need(type(h) is int and h in a.HORIZONS,'HORIZON'); need((sid,h) not in seen_labels,'DUPLICATE_LABEL'); seen_labels.add((sid,h))
        a.label(samples[sid],label,h,cutoff)
        results.append({'sampleId':sid,'horizonMs':h,'firstRecordedBook':first_recorded(samples[sid],label,originals[sid],events)})
    for p,s,b in pins: need(signature(p)==s and sha(p.read_bytes())==sha(b),'SOURCE_CHANGED')
    need(signature(manifest)==before and manifest.read_bytes()==raw,'MANIFEST_CHANGED')
    return {'schemaVersion':1,'manifestSha256':expected,'cohortId':m['cohortId'],'sampleMembershipVerified':len(samples),'labelRecordedOrder':results,
            'semanticsLfSha256':{'verifier':sha(Path(__file__).read_bytes().replace(b'\r\n',b'\n')),'analyzer':sha(Path(a.__file__).read_bytes().replace(b'\r\n',b'\n')),'observerNormalization':sha(Path(o.__file__).read_bytes().replace(b'\r\n',b'\n'))},
            'tournamentContext':'unavailable_original_topic_or_control_binding_missing','resolutionEquivalence':'unavailable_not_verified',
            'status':'INCOMPLETE_ORIGINAL_WITNESS','originalSigAndTradeMembership':'verified_for_retained_samples' if samples else 'unavailable_no_samples','observerFirstAvailability':'unavailable_original_consumption_trace_missing',
            'firstConsumedEligibleBook':'unavailable_original_consumption_trace_missing','collectorProcessingClockAndEpoch':'unavailable_not_in_raw_trade_receipts',
            'externalPriceAnchorMembership':'unavailable_not_checked_by_this_module','rotationContinuity':'unavailable_independent_segment_chains',
            'wholeWitnessVerified':False,'eligibleForModelValidation':False,'executionAuthority':False,'providerTapeComplete':False,'accountRequests':0}


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--manifest',required=True,type=Path);p.add_argument('--manifest-sha256',required=True);p.add_argument('--output',required=True,type=Path)
    args=p.parse_args();result=verify(args.manifest,args.manifest_sha256)
    with args.output.open('x',encoding='utf-8') as out:out.write(json.dumps(result,indent=2,sort_keys=True)+'\n')
