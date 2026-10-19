"""Original public realtime frames, without the production strategy's filters.

Only explicitly authorized, already inspected development calendars are read.
Full-file verification is the remote extractor's recorded evidence; selected
original byte hashes and contiguous per-role chain slices are verified locally.
Neither those hashes nor these snapshots prove continuous exchange truth.
"""
import base64,gzip,hashlib,json,re
from collections import Counter
from decimal import Decimal
from pathlib import Path
from moloch.research.verify_learning_transport import source_hash
from moloch.research.compile_native_learning_history import amount,levels
from moloch.learning_v2.reference_adapters import millis
from moloch.learning_v2.liquidity import Book
from moloch.learning_flow.features import trade

ORIGIN='original_official_sig_realtime_frame'


def wire_amount(value):
    # Match source.rs decimal(): documented .005 ticks, allowing only tiny
    # JSON binary-float serialization noise (e.g. .41000000000000003).
    if type(value)is float:
        ticks=Decimal(str(value))*200;nearest=ticks.to_integral_value()
        if not ticks.is_finite()or not 0<ticks<200 or abs(ticks-nearest)>Decimal('0.0000001'):
            raise ValueError('ORIGINAL_REALTIME_TICK_PRICE')
        return int(nearest)*5000
    return amount(value)


def wire_levels(values,bid):
    return levels([{**v,'price':format(Decimal(wire_amount(v['price']))/1000000,'f')}for v in values],bid)


def frames(directory,archive_sha256,manifest_sha256):
    root=Path(directory);p=root/'original-slices.jsonl.gz';raw=(root/'source-manifest.json').read_bytes()
    if hashlib.sha256(raw).hexdigest()!=manifest_sha256:raise ValueError('BREADTH_MANIFEST_CHANGED')
    raw_manifest=raw;manifest=json.loads(raw_manifest)
    if hashlib.sha256(p.read_bytes()).hexdigest()!=archive_sha256 or manifest['archive_sha256']!=archive_sha256:
        raise ValueError('BREADTH_ARCHIVE_CHANGED')
    roles=manifest['roles_ms'];sources={s['file']:s for s in manifest['originals']};last={};counts=Counter();clock=-1
    with gzip.open(p,'rb')as f:
        for line in f:
            envelope=json.loads(line);name=envelope['file'];role=envelope['role']
            if envelope['kind']!='original_breadth_row'or name not in sources or role not in('fit','search'):
                raise ValueError('EXPLICIT_AUTHORIZED_BREADTH_SLICE')
            raw=base64.b64decode(envelope['raw_base64'],validate=True)
            if len(raw)>16*1024*1024 or hashlib.sha256(raw).hexdigest()!=envelope['raw_sha256']:
                raise ValueError('BREADTH_ORIGINAL_BYTES')
            row=json.loads(raw);source_hash(raw,row);start=envelope['start_offset'];end=envelope['end_offset']
            source=sources[name]
            if not 0<=start<end<=source['bytes']or end-start!=len(raw)or not source['first_sequence']<=row['sequence']<=source['last_sequence']:
                raise ValueError('BREADTH_ORIGINAL_FILE_BOUNDS')
            key=(name,role)
            if key in last:
                offset,seq,previous=last[key]
                if start!=offset or row['sequence']!=seq+1 or row['previous']!=previous:
                    raise ValueError('BREADTH_ROLE_CHAIN_DISCONTINUITY')
            last[key]=(end,row['sequence'],row['hash'])
            at=millis(row['event']['receivedAt']);lo,hi=roles[role]
            if not lo<=at<hi or at<clock:raise ValueError('BREADTH_AUTHORIZED_CLOCK')
            clock=at;counts[role]+=1
            yield row['event'],row['hash'],at,role
    if dict(counts)!=manifest['counts']:raise ValueError('BREADTH_AUTHORIZED_SLICE_INCOMPLETE')
    if hashlib.sha256(p.read_bytes()).hexdigest()!=archive_sha256 or(root/'source-manifest.json').read_bytes()!=raw_manifest:
        raise ValueError('BREADTH_SOURCE_CHANGED_DURING_READ')


def identifier(value):
    text=str(value)
    if type(value)is bool or not re.fullmatch(r'[0-9]+',text):raise ValueError('BREADTH_NUMERIC_SCOPE')
    return text


class Decoder:
    def __init__(self,tournament):
        self.tournament=tournament;self.revisions={};self.books={};self.mapping={};self.counts=Counter()

    def decode(self,event,original,at,role):
        if event.get('source')!='official_sig_realtime'or millis(event['receivedAt'])!=at:
            raise ValueError('BREADTH_OFFICIAL_RECEIVER_CLOCK')
        common=dict(venue='SIG',available_at_ms=at,received_at_ms=at,
            original_realtime_received_at_ms=at,snapshot_origin=ORIGIN,original_sha256=original,calendar_role=role)
        if event['event']=='stream_reset':
            self.revisions.clear();self.counts['stream_resets']+=1
            return [{**common,'kind':'gap','market_id':None,'exchange_id':None,'gap_reason':'original_stream_reset'}],[]
        if event['event']!='markets_batch':raise ValueError('UNSUPPORTED_ORIGINAL_BREADTH_FRAME')
        rows=[];prints=[];seen=set()
        for market in event['payload']['markets']:
            mid=identifier(market['marketId']);batch=market['batch'];delivery=batch['delivery']
            if mid in seen:raise ValueError('BREADTH_DUPLICATE_MARKET')
            seen.add(mid);revision=delivery['revision'];previous=delivery['previousRevision']
            if delivery['model']!='best-effort-authoritative-resync'or any(type(x)is not int for x in(revision,previous))or not 0<=previous<revision:
                raise ValueError('BREADTH_DELIVERY_REVISION')
            old=self.revisions.get(mid)
            if old is not None and revision<=old:
                self.counts['duplicate_delivery']+=1;continue
            self.revisions[mid]=revision
            compact=batch.get('resyncRequired',False)
            if type(compact)is not bool:raise ValueError('BREADTH_RESYNC_FLAG')
            if compact or(old is not None and previous>old):
                rows.append({**common,'kind':'gap','market_id':mid,'exchange_id':None,'gap_reason':'original_delivery_resync'})
            exchanges=set()
            for book in batch.get('books',[]):
                ex=identifier(book['exchangeId']);source=millis(book['asOf']['at']);seq=identifier(book['asOf']['sequence'])
                if ex in exchanges or source>at:raise ValueError('BREADTH_BOOK_SCOPE_OR_CLOCK')
                exchanges.add(ex)
                if ex in self.mapping and self.mapping[ex]!=mid:raise ValueError('BREADTH_EXCHANGE_MARKET_CHANGED')
                self.mapping[ex]=mid
                bids=wire_levels(book['bids'],True);asks=wire_levels(book['asks'],False)
                Book(ex,at,source,tuple(map(tuple,bids)),tuple(map(tuple,asks)),original)
                stamp=(int(seq),source);fact=(bids,asks);prior=self.books.get(ex)
                if prior is not None:
                    if stamp<prior[0]:self.counts['older_book_ignored']+=1;continue
                    if stamp==prior[0]and fact!=prior[1]:raise ValueError('BREADTH_SAME_SOURCE_BOOK_CONFLICT')
                self.books[ex]=(stamp,fact)
                rows.append({**common,'kind':'book','market_id':mid,'exchange_id':ex,'source_at_ms':source,
                    'source_clock_semantics':'original_realtime_asOf_at','sequence':seq,'bids':bids,'asks':asks})
                self.counts['books']+=1
                if not bids or not asks:self.counts['empty_sided_books']+=1
            for t in batch.get('trades',[]):
                ex=identifier(t['exchangeId'])
                if identifier(t['marketId'])!=mid or t['tournamentId']!=self.tournament or self.mapping.get(ex,mid)!=mid:
                    raise ValueError('BREADTH_TRADE_SCOPE')
                if t.get('id')is None or t.get('sequence')is None:
                    self.counts['unsequenced_prints_not_added']+=1;continue
                identifier(t['sequence']);price=wire_amount(t['price'])
                e=dict(type='trade',tournamentId=self.tournament,marketId=mid,exchangeId=ex,
                    id=str(t['id']),idNamespace='sig_engine_match',executedAt=t['executedAt'],receivedAt=event['receivedAt'],
                    quantity=t['quantity'],yesPrice=format(Decimal(price)/1000000,'f'))
                prints.append(trade(e,original,at));self.counts['reported_engine_prints']+=1
            for settled in batch.get('marketSettled',[]):
                if identifier(settled['marketId'])!=mid or settled['tournamentId']!=self.tournament:
                    raise ValueError('BREADTH_SETTLEMENT_SCOPE')
                rows.append({**common,'kind':'gap','market_id':mid,'exchange_id':identifier(settled['exchangeId']),
                    'gap_reason':'settlement_requires_lifecycle_replay'})
        return rows,prints
