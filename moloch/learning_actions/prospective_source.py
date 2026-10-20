"""Normalize already-verified public capture records, never an API client.

The caller must verify original file/producer hashes before invoking this
decoder and provide an authorized development calendar. REST receipts describe
observed snapshots, not WebSocket broadcasts or continuous exchange truth.
"""
import re
from urllib.parse import urlsplit,parse_qs
from .breadth_source import Decoder,identifier,wire_levels,ORIGIN
from .snapshot_clock import REST_ORIGIN
from moloch.learning_v2.reference_adapters import millis
from moloch.learning_v2.liquidity import Book


class ProspectiveDecoder:
    def __init__(self,tournament,bounds,role,*,subscription_binding=None):
        if (role not in('fit','search')or len(bounds)!=2
                or any(type(v)is not int or v<0 for v in bounds)or bounds[0]>=bounds[1]):
            raise ValueError('AUTHORIZED_DEVELOPMENT_CAPTURE_CALENDAR_REQUIRED')
        if subscription_binding is not None:
            if (set(subscription_binding)!={'tournamentId','manifestSha256'}
                    or subscription_binding['tournamentId']!=tournament
                    or re.fullmatch('[0-9a-f]{64}',subscription_binding['manifestSha256'])is None):
                raise ValueError('VERIFIED_CAPTURE_SESSION_SUBSCRIPTION_BINDING')
        self.subscription_binding=subscription_binding
        self.decoder=Decoder(tournament);self.bounds=tuple(bounds);self.role=role;self.last=-1

    def common(self,event,original,origin):
        if type(original)is not str or re.fullmatch('[0-9a-f]{64}',original)is None:
            raise ValueError('VERIFIED_ORIGINAL_RECORD_HASH_REQUIRED')
        at=millis(event['receivedAt'])
        if not self.bounds[0]<=at<self.bounds[1]or at<self.last:
            raise ValueError('ORIGINAL_CAPTURE_RECEIPT_CALENDAR_OR_ORDER')
        tournament=event.get('tournamentId')
        if tournament is None and event.get('kind')=='subscribed'and self.subscription_binding is not None:
            tournament=self.subscription_binding['tournamentId']
        if tournament!=self.decoder.tournament:raise ValueError('CAPTURE_TOURNAMENT_SCOPE')
        self.last=at
        return dict(venue='SIG',available_at_ms=at,received_at_ms=at,
            snapshot_origin=origin,original_sha256=original,calendar_role=self.role)

    def websocket(self,event,original):
        if event.get('source')!='official_sig_realtime':raise ValueError('ORIGINAL_PUBLIC_WEBSOCKET_SOURCE')
        common=self.common(event,original,ORIGIN);at=common['available_at_ms']
        common['original_realtime_received_at_ms']=at;mid=identifier(event['marketId']);d=self.decoder
        if event['kind']=='subscribed':
            d.revisions.pop(mid,None)
            d.needs_baseline.update(ex for ex,market in d.mapping.items()if market==mid)
            d.counts['original_topic_subscriptions']+=1
            return [{**common,'kind':'gap','market_id':mid,'exchange_id':None,
                'gap_reason':'original_topic_subscription_requires_baseline',
                'subscription_session_manifest_sha256':None if self.subscription_binding is None else self.subscription_binding['manifestSha256'],
                'authoritative_market_refetch_required':True}],[]
        if event['kind']!='broadcast'or event['event']!='market_batch':
            raise ValueError('SUPPORTED_ORIGINAL_PUBLIC_BROADCAST_REQUIRED')
        for key in('duplicate','gap','requiresRestResync'):
            if type(event.get(key))is not bool:raise ValueError('EXPLICIT_ORIGINAL_TRANSPORT_FLAGS')
        gap=[]
        if event['gap']or event['requiresRestResync']:
            d.needs_baseline.update(ex for ex,market in d.mapping.items()if market==mid)
            gap=[{**common,'kind':'gap','market_id':mid,'exchange_id':None,
                'gap_reason':'original_transport_requires_resync',
                'authoritative_market_refetch_required':True}]
        wrapped=dict(source='official_sig_realtime',receivedAt=event['receivedAt'],event='markets_batch',
            payload={'markets':[{'marketId':mid,'batch':event['payload']}]})
        rows,prints=d.decode(wrapped,original,at,self.role)
        for row in rows:
            row['original_transport_gap']=event['gap']
            row['original_requires_rest_resync']=event['requiresRestResync']
        d.counts['original_broadcasts']+=1
        return gap+rows,prints

    def rest_book(self,event,original):
        if event.get('source')!='official_sig_api_v1'or event.get('kind')!='book'or event.get('status')!=200:
            raise ValueError('ORIGINAL_SUCCESSFUL_PUBLIC_REST_BOOK_REQUIRED')
        common=self.common(event,original,REST_ORIGIN);at=common['available_at_ms'];mid=identifier(event['marketId'])
        endpoint=urlsplit(event['endpoint']);query=parse_qs(endpoint.query,keep_blank_values=True)
        if (endpoint.scheme or endpoint.netloc or endpoint.fragment or endpoint.path!=f'/markets/{mid}/orderbook'
                or query!={'tournamentId':[self.decoder.tournament],'depth':['200']}):
            raise ValueError('EXACT_RECORDED_PUBLIC_BOOK_ENDPOINT')
        started=millis(event['requestStartedAt'])
        if started>at:raise ValueError('ORIGINAL_REST_REQUEST_RECEIPT_CLOCK')
        common.update(original_rest_received_at_ms=at,original_request_started_at_ms=started)
        contexts=[c for c in event['data']['contexts']if c['type']=='tournament'
            and c['tournament']['id']==self.decoder.tournament]
        if len(contexts)!=1:raise ValueError('ONE_EXACT_TOURNAMENT_BOOK_CONTEXT')
        d=self.decoder;rows=[];seen=set()
        for book in contexts[0]['orderbook']['exchanges']:
            ex=identifier(book['exchangeId']);source=millis(book['asOf']['at']);seq=identifier(book['asOf']['sequence'])
            if ex in seen or source>at:raise ValueError('REST_BOOK_SCOPE_OR_SOURCE_CLOCK')
            seen.add(ex)
            if ex in d.mapping and d.mapping[ex]!=mid:raise ValueError('BREADTH_EXCHANGE_MARKET_CHANGED')
            bids=wire_levels(book['bids'],True);asks=wire_levels(book['asks'],False)
            Book(ex,at,source,tuple(map(tuple,bids)),tuple(map(tuple,asks)),original)
            stamp=(int(seq),source);fact=(bids,asks);prior=d.books.get(ex)
            if prior is not None:
                if stamp<prior[0]:d.counts['older_rest_book_ignored']+=1;continue
                if stamp==prior[0]and fact!=prior[1]:raise ValueError('BREADTH_SAME_SOURCE_BOOK_CONFLICT')
                if stamp==prior[0]and ex not in d.needs_baseline:
                    d.counts['same_version_rest_book_ignored']+=1;continue
            d.books[ex]=(stamp,fact);d.mapping[ex]=mid;d.needs_baseline.discard(ex)
            rows.append({**common,'kind':'book','market_id':mid,'exchange_id':ex,'source_at_ms':source,
                'source_clock_semantics':'original_REST_exchange_asOf_at','sequence':seq,'bids':bids,'asks':asks})
            d.counts['rest_books']+=1
        if not seen:
            rows.append({**common,'kind':'gap','market_id':mid,'exchange_id':None,
                'gap_reason':'original_empty_REST_exchange_context','authoritative_market_refetch_required':True})
        d.counts['original_rest_book_receipts']+=1
        return rows,[]
