import copy
import hashlib
import json
import unittest
from .reference_adapters import kalshi_references,polymarket_references


def receipt(body):
    text=json.dumps(body);return {'status':200,'requestStartedAt':'2026-10-19T04:15:00Z',
        'receivedAt':'2026-10-19T04:15:01Z','bodyText':text,'bodySha256':hashlib.sha256(text.encode()).hexdigest(),'body':body}


def raw(row):return (json.dumps(row)+'\n').encode()
AT=1792383302000


class ReferenceOrientationTests(unittest.TestCase):
    def test_kalshi_no_mapping_inverts_the_whole_interval(self):
        r={**receipt({'orderbook_fp':{'yes_dollars':[['0.4','12.5']],'no_dollars':[['0.5','20']]}}),
           'ticker':'party','mappingKind':'direct_party_reference','sigMarketIds':['1']}
        x=kalshi_references(raw(r),[{'sigMarketId':'1','side':'NO'}],AT)[0]
        self.assertEqual((x['bid_yes_micros'],x['ask_yes_micros'],x['price_yes_micros']),(500000,600000,550000))
        self.assertFalse(x['pricing_authority']);self.assertFalse(x['resolution_equivalence_verified'])
        r['mappingKind']='analog';self.assertEqual(kalshi_references(raw(r),[],AT),[])

    def poly(self):
        return {'identity':{'assetId':'token','conditionId':'question','outcome':'No','sigReferences':[
            {'sigMarketId':'1','sigSide':'YES','relationship':'same_party_conditional'},
            {'sigMarketId':'2','sigSide':'NO','relationship':'same_party_conditional'},
            {'sigMarketId':'3','sigSide':None,'relationship':'race_context_only'}]},
            'receipt':receipt({'asset_id':'token','market':'question','timestamp':str(AT-1000),
                'bids':[{'price':'0.4','size':'5'}],'asks':[{'price':'0.42','size':'9'}]})}

    def test_token_no_and_sig_no_each_invert_once_context_is_excluded(self):
        xs=polymarket_references(raw(self.poly()),AT)
        self.assertEqual([x['market_id']for x in xs],['1','2'])
        self.assertEqual([(x['bid_yes_micros'],x['ask_yes_micros'])for x in xs],[(580000,600000),(400000,420000)])

    def test_changed_original_bytes_or_wrong_token_cannot_supply_a_reference(self):
        p=self.poly();p['receipt']['bodyText']+=' '
        with self.assertRaisesRegex(ValueError,'REFERENCE_BODY_BYTES_CHANGED'):polymarket_references(raw(p),AT)
        p=self.poly();p['identity']['assetId']='other'
        with self.assertRaisesRegex(ValueError,'REFERENCE_TOKEN_IDENTITY'):polymarket_references(raw(p),AT)

    def test_future_source_and_receipt_clocks_are_rejected(self):
        p=self.poly();body=copy.deepcopy(p['receipt']['body']);body['timestamp']=str(AT+1);p['receipt']=receipt(body)
        with self.assertRaises(ValueError):polymarket_references(raw(p),AT)
        with self.assertRaisesRegex(ValueError,'REFERENCE_AVAILABILITY_CLOCK'):polymarket_references(raw(self.poly()),AT-2000)

    def test_empty_book_invalidates_reference_without_inventing_a_price(self):
        p=self.poly();body=copy.deepcopy(p['receipt']['body']);body['bids']=[];p['receipt']=receipt(body)
        xs=polymarket_references(raw(p),AT)
        self.assertEqual([x['market_id']for x in xs],['1','2'])
        self.assertTrue(all(x['kind']=='reference_gap'and 'price_yes_micros'not in x for x in xs))

    def test_explicit_gap_policy_preserves_future_clock_without_inventing_price(self):
        p=self.poly();body=copy.deepcopy(p['receipt']['body']);body['timestamp']=str(AT+2);p['receipt']=receipt(body)
        rows=polymarket_references(raw(p),AT,invalid_market_data='gap')
        self.assertEqual([r['market_id']for r in rows],['1','2'])
        self.assertTrue(all(r['kind']=='reference_gap' and r['reason']=='REFERENCE_SOURCE_CLOCK_AFTER_RECEIPT'
                            and r['rejected_original_source_at_ms']==AT+2 and 'price_yes_micros'not in r for r in rows))
        p['receipt']['bodyText']+=' '
        with self.assertRaisesRegex(ValueError,'REFERENCE_BODY_BYTES_CHANGED'):
            polymarket_references(raw(p),AT,invalid_market_data='gap')

    def test_crossed_book_gap_never_becomes_spread_or_relaxes_identity_check(self):
        p=self.poly();body=copy.deepcopy(p['receipt']['body']);body['bids'][0]['price']='0.9';p['receipt']=receipt(body)
        rows=polymarket_references(raw(p),AT,invalid_market_data='gap')
        self.assertTrue(all(r['kind']=='reference_gap'and r['reason']=='REFERENCE_CROSSED_BOOK'for r in rows))
        p['identity']['assetId']='other'
        with self.assertRaisesRegex(ValueError,'REFERENCE_TOKEN_IDENTITY'):
            polymarket_references(raw(p),AT,invalid_market_data='gap')


if __name__=='__main__':unittest.main()
