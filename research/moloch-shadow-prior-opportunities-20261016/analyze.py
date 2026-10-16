"""Finite, read-only market-implied-prior level-gap screen. No EV authority."""
import datetime as dt
from decimal import Decimal
import hashlib
import json
from pathlib import Path


def ms(value):
    t=dt.datetime.fromisoformat(value.replace('Z','+00:00'))
    if t.tzinfo is None:raise ValueError('timezone')
    return int(t.timestamp()*1000)


def amount(value):
    n=Decimal(str(value));u=n*1000000
    if not n.is_finite() or u!=int(u):raise ValueError('decimal')
    return int(u)


def sig_levels(rows,bids):
    out=[]
    for row in rows:
        raw=Decimal(str(row['price']));p=int((raw/Decimal('.005')).quantize(Decimal(1)))*5000
        if abs(raw-Decimal(p)/1000000)>Decimal('0.000000000001'):raise ValueError('tick')
        q=amount(row['quantity'])
        if not 0<p<1000000 or q<=0 or (out and (p>=out[-1][0] if bids else p<=out[-1][0])):raise ValueError('depth')
        out.append((p,q))
    return out


def original_books(path,start_offset=0):
    current={};count=0;fragments=0
    with Path(path).open('rb') as f:
        if start_offset:
            f.readline();fragments+=1
        for raw in f:
            if not raw.endswith(b'\n'):fragments+=1;continue
            count+=1
            if len(raw)>4000000:raise ValueError('line bound')
            r=json.loads(raw);e=r['event']
            if e.get('source')!='official_sig_realtime' or e.get('event')!='markets_batch':continue
            for m in e['payload']['markets']:
                for b in m['batch'].get('books',[]):
                    key=(str(m['marketId']),str(b['exchangeId']))
                    v={'book':b,'receivedAt':e['receivedAt'],'originalRowSha256':hashlib.sha256(raw).hexdigest(),'journalHash':r['hash'],'line':count,'resyncRequired':m['batch'].get('resyncRequired'),'marketSettled':m['batch'].get('marketSettled'),'delivery':m['batch'].get('delivery')}
                    old=current.get(key)
                    if old is None or (int(b['asOf']['sequence']),b['asOf']['at'])>(int(old['book']['asOf']['sequence']),old['book']['asOf']['at']):current[key]=v
    return current,count,fragments


def gaps(sig_bid,sig_ask,kalshi_bid,kalshi_ask,side):
    buy=sig_ask if side=='YES' else 1000000-sig_bid
    prior=kalshi_bid if side=='YES' else 1000000-kalshi_ask
    return {'sigBuyAskMicros':buy,'hypotheticalConservativePriorMicros':prior,'rawApparentGapMicros':prior-buy,
            'basisSensitivityNetMicros':{str(c):prior-buy-c for c in [5000,20000,50000,100000]}}


def run(folder,manifest_path):
    folder=Path(folder);manifest=json.loads(Path(manifest_path).read_bytes())
    for row in manifest['inputs']:
        if row['bytes']>32000000:raise ValueError('finite input bound')
        b=Path(row['path']).read_bytes()
        if len(b)!=row['bytes'] or hashlib.sha256(b).hexdigest()!=row['sha256']:raise ValueError('input pin')
    paths={r['name']:Path(r['path']) for r in manifest['inputs']}
    mapping=json.loads(paths['mapping'].read_bytes());rest=json.loads(paths['rest-cache'].read_bytes());ws=json.loads(paths['ws-cache'].read_bytes())
    books={};lines=0;fragments=0
    for source in manifest['inputs']:
        if not source['name'].startswith('sig-breadth-suffix-'):continue
        values,n,parts=original_books(source['path'],source['startOffset']);lines+=n;fragments+=parts
        for key,value in values.items():
            value['sourceArtifact']=source['artifact'];old=books.get(key)
            if old is None or (int(value['book']['asOf']['sequence']),value['book']['asOf']['at'])>(int(old['book']['asOf']['sequence']),old['book']['asOf']['at']):books[key]=value
    scope=json.loads(paths['scope'].read_bytes())['scope']
    now=ms(manifest['asOfUtc']);rows=[];counts={'sigOriginalRows':lines,'boundaryFragmentsExcluded':fragments,'sigBookScopesObserved':len(books),'directMappings':0,'missingSigBook':0,'staleSig':0,'missingFreshExternal':0,'malformedBook':0,'settledSig':0}
    wm={r['ticker']:r for r in ws['markets']};rm={r['ticker']:r for r in rest['markets']}
    for ref in mapping['markets']:
        for match in ref['sigMappings']:
            counts['directMappings']+=1
            market=match['sigMarketId'];available=[(key,v) for key,v in books.items() if key[0]==market]
            if len(available)!=1 or match['side']!='YES':counts['missingSigBook']+=1;continue
            (market,ex),source=available[0];b=source['book'];source_at=ms(b['asOf']['at']);received=ms(source['receivedAt'])
            if scope['exchanges'].get(ex)!=market:raise ValueError('current SIG scope conflict')
            if source['marketSettled'] is True:counts['settledSig']+=1;continue
            if not source_at<=received<=now or now-source_at>10000:counts['staleSig']+=1;continue
            ticker=ref['ticker'];external=wm.get(ticker);kind='websocket'
            if external is None or not external.get('valid') or not 0<=now-ms(external['receivedAt'])<=20000 or (external.get('sourceTsMs') is not None and not 0<=now-external['sourceTsMs']<=20000):external=rm.get(ticker);kind='rest'
            if external is None or not 0<=now-ms(external['receivedAt'])<=20000:counts['missingFreshExternal']+=1;continue
            if external.get('ticker')!=ticker or market not in external.get('sigMarketIds',[]) or external.get('mappingKind')!='direct_party_reference':raise ValueError('external mapping conflict')
            try:
                sb=sig_levels(b['bids'],True);sa=sig_levels(b['asks'],False);n=external['normalized'];kb=[(amount(l['price']),amount(l['quantity'])) for l in n['yesBids']];ka=[(amount(l['price']),amount(l['quantity'])) for l in n['yesAsks']]
                if not sb or not sa or not kb or not ka or sb[0][0]>=sa[0][0] or kb[0][0]>ka[0][0]:raise ValueError('book')
            except (ValueError,KeyError,TypeError):counts['malformedBook']+=1;continue
            for side in ['YES','NO']:
                v=gaps(sb[0][0],sa[0][0],kb[0][0],ka[0][0],side);sig_depth=sa if side=='YES' else [(1000000-p,q) for p,q in sb]
                rows.append(dict(v,marketId=market,exchangeId=ex,ticker=ticker,office=ref['office'],state=ref.get('state'),party=ref['party'],side=side,mappingKind='direct_party_reference',resolutionEquivalenceVerified=False,sourceKind=kind,sigSourceAt=b['asOf']['at'],sigReceivedAt=source['receivedAt'],sigSourceAgeMs=now-source_at,externalReceivedAt=external['receivedAt'],externalReceiptAgeMs=now-ms(external['receivedAt']),externalSourceTsMs=external.get('sourceTsMs'),externalSourceAgeKnown=external.get('sourceTsMs') is not None,externalFirstAvailableAtMs=None,localExportAvailableAt=next(r['receivedAt'] for r in manifest['inputs'] if r['name']==('ws-cache' if kind=='websocket' else 'rest-cache')),firstAvailabilityLimitation='Exporter original first availability unavailable; only this export local receipt is pinned.',externalBookHash=external.get('stateChainSha256',external.get('bodySha256')),sigOriginalRowSha256=source['originalRowSha256'],sigJournalHash=source['journalHash'],sigAskTopQuantityMicros=sig_depth[0][1],kalshiPriorBidQuantityMicros=(kb if side=='YES' else ka)[0][1],kalshiYesBidMicros=kb[0][0],kalshiYesAskMicros=ka[0][0],kalshiYesBidQuantityMicros=kb[0][1],kalshiYesAskQuantityMicros=ka[0][1],hypotheticalPriorIntervalMicros=[(kb[0][0] if side=='YES' else 1000000-ka[0][0]),(ka[0][0] if side=='YES' else 1000000-kb[0][0])],sigAskQuantityThroughBasisMicros={str(c):sum(q for p,q in sig_depth if p<v['hypotheticalConservativePriorMicros']-c) for c in [5000,20000,50000,100000]},sigResyncRequired=source['resyncRequired'],sigDelivery=source['delivery'],sigSourceArtifact=source['sourceArtifact'],sigOriginalLine=source['line'],completeTradeTapeVerified=False,continuityVerified=False))
    rows.sort(key=lambda r:(-r['rawApparentGapMicros'],r['marketId'],r['side']))
    return {'asOfUtc':manifest['asOfUtc'],'sourceCodeSha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'inputManifestSha256':hashlib.sha256(Path(manifest_path).read_bytes()).hexdigest(),'counts':counts,'ageScreenedSideComparisons':len(rows),'positiveAtBasisCounts':{str(c):sum(r['rawApparentGapMicros']>c for r in rows) for c in [5000,20000,50000,100000]},'topCandidates':rows[:12],'feesAssumedMicros':0,'calibratedExpectedValue':False,'executionAuthority':False,'unconditionalPayoutFloor':False,'limitations':['Partial finite SIG suffix; provider continuity and source topic binding unknown.','Market-implied bid is a hypothetical prior, not true probability, semantic equivalence or hedge.','Contract cancellation/refund/fusion/independent/replacement/runoff/timing risks remain unpriced beyond explicit arbitrary basis sensitivities.','Depth is displayed snapshot capacity, not a fill or liquidation guarantee.','Three analog mappings excluded.']}


if __name__=='__main__':
    import sys
    print(json.dumps(run(Path(sys.argv[1]).parent,sys.argv[1]),indent=2,sort_keys=True))
