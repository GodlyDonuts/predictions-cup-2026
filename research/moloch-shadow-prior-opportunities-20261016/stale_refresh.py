"""Stale SIG levels nominate READS ONLY. Does not modify the frozen current report."""
from analyze import amount,gaps,ms,original_books,sig_levels
import hashlib,json,sys
from pathlib import Path


def run(path):
    path=Path(path);m=json.loads(path.read_bytes());inputs={r['name']:r for r in m['inputs']}
    for r in inputs.values():
        b=Path(r['path']).read_bytes();assert len(b)==r['bytes'] and hashlib.sha256(b).hexdigest()==r['sha256']
    now=ms(m['asOfUtc']);mapping=json.loads(Path(inputs['mapping']['path']).read_bytes());rest={r['ticker']:r for r in json.loads(Path(inputs['rest-cache']['path']).read_bytes())['markets']};ws={r['ticker']:r for r in json.loads(Path(inputs['ws-cache']['path']).read_bytes())['markets']};books={}
    scope=json.loads(Path(inputs['scope']['path']).read_bytes())['scope']['exchanges']
    for r in inputs.values():
        if r['name'].startswith('sig-breadth-suffix-'):
            for k,v in original_books(r['path'],r['startOffset'])[0].items():
                old=books.get(k)
                if old is None or (int(v['book']['asOf']['sequence']),v['book']['asOf']['at'])>(int(old['book']['asOf']['sequence']),old['book']['asOf']['at']):books[k]=v
    rows=[];seen=set()
    for ref in mapping['markets']:
        for match in ref['sigMappings']:
            candidates=[(k,v)for k,v in books.items()if k[0]==match['sigMarketId']]
            if len(candidates)!=1 or match['side']!='YES':continue
            (market,ex),source=candidates[0];b=source['book'];age=now-ms(b['asOf']['at'])
            if age<=10000 or source['marketSettled'] is True or not ms(b['asOf']['at'])<=ms(source['receivedAt'])<=now:continue
            assert scope[ex]==market
            ext=ws.get(ref['ticker']);kind='websocket'
            if ext is None or not ext.get('valid') or not 0<=now-ms(ext['receivedAt'])<=20000 or (ext.get('sourceTsMs') is not None and not 0<=now-ext['sourceTsMs']<=20000):ext=rest.get(ref['ticker']);kind='rest'
            if ext is None or not 0<=now-ms(ext['receivedAt'])<=20000:continue
            assert ext['mappingKind']=='direct_party_reference' and market in ext['sigMarketIds']
            bids=sig_levels(b['bids'],True);asks=sig_levels(b['asks'],False);n=ext['normalized']
            if not bids or not asks or not n['yesBids'] or not n['yesAsks']:continue
            seen.add(market)
            for side in ['YES','NO']:
                g=gaps(bids[0][0],asks[0][0],amount(n['yesBids'][0]['price']),amount(n['yesAsks'][0]['price']),side)
                rows.append(dict(g,marketId=market,exchangeId=ex,ticker=ref['ticker'],side=side,sigSourceAt=b['asOf']['at'],sigReceivedAt=source['receivedAt'],sigSourceAgeMs=age,externalReceivedAt=ext['receivedAt'],externalSourceKind=kind,externalBookHash=ext.get('stateChainSha256',ext.get('bodySha256')),sigOriginalRowSha256=source['originalRowSha256'],classification='stale_SIG_REFRESH_READ_only',currentlyExecutable=False,executionAuthority=False))
    rows.sort(key=lambda r:(-r['rawApparentGapMicros'],r['marketId'],r['side']))
    return {'asOfUtc':m['asOfUtc'],'inputManifestSha256':hashlib.sha256(path.read_bytes()).hexdigest(),'sourceCodeSha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'staleDirectMarketsCompared':len(seen),'staleSideComparisons':len(rows),'positiveAtBasisCounts':{str(c):sum(r['rawApparentGapMicros']>c for r in rows)for c in [5000,20000,50000,100000]},'topStaleReadNominations':rows[:12],'financialFreshnessPassed':False,'executionAuthority':False,'warning':'Old SIG levels versus captured newer external cache are asynchronous READ nominations, not current opportunities, fills or estimated profit.'}


if __name__=='__main__':print(json.dumps(run(sys.argv[1]),indent=2,sort_keys=True))
