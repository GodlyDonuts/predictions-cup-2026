"""Historical public trades via the existing collector's shared admission queue.

No credentials, exchange client, book reconstruction or training admission.
Historical event time and present retrieval time deliberately remain separate.
"""
import argparse,hashlib,json,os,time,urllib.error,urllib.parse,urllib.request
from datetime import datetime,timezone
from decimal import Decimal,InvalidOperation
from pathlib import Path


def digest(raw):return hashlib.sha256(raw).hexdigest()
def canonical(value):return (json.dumps(value,sort_keys=True,separators=(',',':'))+'\n').encode()
def instant(value):
    d=datetime.fromisoformat(value.replace('Z','+00:00'))
    if d.utcoffset() is None:raise ValueError('EXPLICIT_UTC_OFFSET_REQUIRED')
    return int(d.timestamp()*1000)


def suffix(ticker,start,end,cursor=''):
    if not ticker or any(x not in 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-' for x in ticker):
        raise ValueError('EXACT_CATALOG_TICKER_REQUIRED')
    if type(start)is not int or type(end)is not int or not 0<=start<end:
        raise ValueError('HALF_OPEN_HISTORY_RANGE_REQUIRED')
    q={'ticker':ticker,'min_ts':start,'max_ts':end,'limit':1000}
    if cursor:q['cursor']=cursor
    return '/markets/trades?'+urllib.parse.urlencode(q)


def validate(receipt,path,ticker,start,end):
    if(receipt.get('source')!='kalshi_official_v2' or receipt.get('status')!=200
       or receipt.get('path')!='/trade-api/v2'+path):raise ValueError('EXACT_OFFICIAL_RECEIPT_REQUIRED')
    raw=receipt['bodyText'].encode()
    if digest(raw)!=receipt['bodySha256'] or json.loads(raw)!=receipt['body']:
        raise ValueError('ORIGINAL_BODY_HASH_OR_PARSED_FACTS_CHANGED')
    requested=instant(receipt['requestStartedAt']);received=instant(receipt['receivedAt'])
    if received<requested:raise ValueError('RECEIPT_CLOCK_REGRESSION')
    body=receipt['body'];rows=body['trades'];cursor=body.get('cursor','')
    if not isinstance(rows,list) or len(rows)>1000 or not isinstance(cursor,str):
        raise ValueError('PAGINATED_TRADE_SCHEMA_REQUIRED')
    clocks=[]
    for r in rows:
        at=instant(r['created_time'])
        # The API may include the exact upper boundary. Preserve it in raw
        # receipts but never silently admit it into a half-open replay split.
        if r['ticker']!=ticker or not start*1000<=at<=end*1000 or at>received:
            raise ValueError('HISTORY_IDENTITY_OR_CLOCK_OUTSIDE_REQUEST')
        if not isinstance(r.get('trade_id'),str) or not r['trade_id']:
            raise ValueError('ORIGINAL_TRADE_ID_REQUIRED')
        try:
            n=Decimal(r['count_fp']);yes=Decimal(r['yes_price_dollars']);no=Decimal(r['no_price_dollars'])
            if not(n.is_finite() and n>0 and yes.is_finite() and no.is_finite() and 0<=yes<=1 and 0<=no<=1):
                raise ValueError('EXACT_FINITE_TRADE_AMOUNTS_REQUIRED')
        except (InvalidOperation,TypeError):raise ValueError('EXACT_FINITE_TRADE_AMOUNTS_REQUIRED') from None
        clocks.append(at)
    return {'records':len(rows),'insideHalfOpenRange':sum(x<end*1000 for x in clocks),
            'firstEventMs':min(clocks)if clocks else None,'lastEventMs':max(clocks)if clocks else None,
            'requestStartedAt':receipt['requestStartedAt'],'receivedAt':receipt['receivedAt'],
            'nextCursor':cursor,'bodySha256':receipt['bodySha256']}


def fetch(path):
    url='http://127.0.0.1:17841/v1/discover?'+urllib.parse.urlencode({'path':path})
    # No redirect can turn this local request into an independent API client.
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self,*args,**kwargs):return None
    with urllib.request.build_opener(NoRedirect).open(url,timeout=45)as response:
        raw=response.read(12*1024*1024+1)
        if len(raw)>12*1024*1024:raise ValueError('LOCAL_RECEIPT_SIZE_LIMIT')
        return json.loads(raw)


def recover(root,plan,*,fetcher=fetch,maximum_pages=0):
    root=Path(root);root.mkdir(parents=True,exist_ok=True);raw=canonical(plan);p=root/'plan.json'
    if p.exists():
        if p.read_bytes()!=raw:raise ValueError('IMMUTABLE_RECOVERY_PLAN_CHANGED')
    else:
        with p.open('xb')as f:f.write(raw)
    pages=root/'pages';pages.mkdir(exist_ok=True);completed=0;records=0;retrieved=0
    def status(state,**extra):
        value={'state':state,'observedUtc':datetime.now(timezone.utc).isoformat(),'planSha256':digest(raw),
               'completedTickers':completed,'totalTickers':len(plan['tickers']),'records':records,
               'pagesRetrievedThisInvocation':retrieved,'trainingAdmitted':False,'orderBooksReconstructed':False,
               'originalHistoricalDeliveryVerified':False,'productionChanged':False,**extra}
        pending=root/'status.pending';pending.write_bytes(canonical(value));pending.replace(root/'status.json')
    status('recovering')
    for i,ticker in enumerate(plan['tickers']):
        cursor='';seen=set();page=0
        while True:
            if(root/'STOP').exists():status('stopped');return
            request=suffix(ticker,plan['startTs'],plan['endTs'],cursor);target=pages/f'{i:04d}-{page:07d}.json'
            if target.exists():receipt=json.loads(target.read_bytes())
            else:
                if maximum_pages and retrieved>=maximum_pages:status('bounded_incomplete');return
                failures=0
                while True:
                    try:receipt=fetcher(request);break
                    except (urllib.error.HTTPError,urllib.error.URLError,TimeoutError)as e:
                        if getattr(e,'code',None)not in(None,409,429,500,502,503,504):raise
                        failures+=1;status('waiting_on_existing_collector',tickerIndex=i,consecutiveFailures=failures)
                        if failures>=60:raise RuntimeError('EXISTING_COLLECTOR_UNAVAILABLE')from None
                        time.sleep(min(30,2**min(failures,5)))
                validate(receipt,request,ticker,plan['startTs'],plan['endTs'])
                pending=target.with_suffix('.pending')
                with pending.open('xb')as f:
                    f.write(canonical(receipt));f.flush();os.fsync(f.fileno())
                # Commit complete receipts only; a crash leaves a visible
                # pending artifact instead of a trusted partial JSON page.
                if target.exists():raise ValueError('RECOVERY_PAGE_ALREADY_COMMITTED')
                pending.rename(target)
                retrieved+=1
            row=validate(receipt,request,ticker,plan['startTs'],plan['endTs']);records+=row['insideHalfOpenRange']
            following=row['nextCursor'];status('recovering',tickerIndex=i,page=page)
            if not following:break
            if following in seen:raise ValueError('REPEATED_HISTORY_CURSOR')
            seen.add(following);cursor=following;page+=1
        completed+=1;status('recovering')
    status('complete_receipt_recovery_not_replay_qualification')


def main():
    a=argparse.ArgumentParser();a.add_argument('--universe',required=True);a.add_argument('--output',required=True)
    a.add_argument('--start',required=True);a.add_argument('--end',required=True);a.add_argument('--maximum-pages',type=int,default=0)
    args=a.parse_args();source=Path(args.universe).read_bytes();tickers=sorted({x['ticker']for x in json.loads(source)['markets']})
    lo=instant(args.start);hi=instant(args.end)
    if lo%1000 or hi%1000 or not lo<hi or args.maximum_pages<0:raise ValueError('EXACT_SECOND_RANGE_REQUIRED')
    plan={'classification':'historical_public_trade_recovery_not_original_book_or_delivery_replay',
          'startTs':lo//1000,'endTs':hi//1000,'tickers':tickers,'universeSha256':digest(source),
          'runnerSha256':digest(Path(__file__).read_bytes()),'transport':'existing_collector_discovery_shared_account_limiter',
          'role':'training_history_recovery_only','competitionStartVerified':False,'independentTestUsed':False}
    recover(args.output,plan,maximum_pages=args.maximum_pages)


if __name__=='__main__':main()
